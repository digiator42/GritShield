# MCP Server Example

A worked example of GritShield's native [Model Context Protocol](https://modelcontextprotocol.io)
server, written to be read. Each file is a self-contained guide to one kind of
capability:

| File | What it teaches |
| --- | --- |
| [`src/tools.rs`](src/tools.rs) | Things an agent can *do*: typed arguments, raw JSON, RBAC, kill switches, error handling |
| [`src/resources.rs`](src/resources.rs) | Read-only *context* an agent can pull in |
| [`src/prompts.rs`](src/prompts.rs) | Parameterised instructions a user picks from a list |
| [`src/main.rs`](src/main.rs) | There is no MCP wiring to do — `Router::new()` mounts it |

## Run it

```bash
cargo run --manifest-path examples/mcp_server/Cargo.toml
```

The transport is mounted on `/mcp`. Talk to it with `curl`:

```bash
# handshake
curl -s localhost:8080/mcp \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{
        "protocolVersion":"2025-06-18","capabilities":{},
        "clientInfo":{"name":"curl","version":"1"}}}'

# call the one tool that needs no role
curl -s localhost:8080/mcp \
  -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call",
       "params":{"name":"server_status","arguments":{}}}'
```

`{"result":{"content":[…],"structuredContent":{…},"isError":false}}` is a
success. Failures come back as JSON-RPC errors with a code you can branch on.

## Why `tools/list` looks almost empty

Every capability in this example except `server_status` declares
`required_role`, and an anonymous HTTP caller has no role. So the engine hides
them:

```bash
curl -s localhost:8080/mcp -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
# {"result":{"tools":[{"name":"server_status", …}]}}

curl -s localhost:8080/mcp -H 'content-type: application/json' \
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/call",
       "params":{"name":"search_incidents","arguments":{"query":"x"}}}'
# {"error":{"code":-32002,"message":"Role 'Operator' is required to invoke 'search_incidents'"}}
```

That is the intended behaviour, not a bug: discovery is filtered by the same
authorisation that guards invocation, so a model is never even shown a tool it
would be denied. To see everything, authenticate — see below.

## Connecting a real MCP client

For a client that speaks streamable HTTP, point it at the `POST /mcp` endpoint:

```json
{
  "mcpServers": {
    "gritshield-example": {
      "type": "http",
      "url": "http://localhost:8080/mcp"
    }
  }
}
```

For a client that wants a server-sent event stream, use `GET /mcp/sse`; the
first event carries an `endpoint` you post messages to:

```json
{
  "mcpServers": {
    "gritshield-example": {
      "type": "sse",
      "url": "http://localhost:8080/mcp/sse"
    }
  }
}
```

## Seeing everything: authentication

The MCP layer does its own authentication, independent of HTTP sessions. Set a
secret and restart the server so its bearer tokens are signed with it:

```bash
export JWT_SECRET=dev-secret
```

Tokens are HS256 over `{ "sub", "role", "exp" }`. Mint one with any HS256 tool
— the server only checks the signature, the claims, and the expiry:

```bash
TOKEN=$(python - <<'PY'
import base64, hmac, hashlib, json, time
def b64(b): return base64.urlsafe_b64encode(b).rstrip(b"=")
h = b64(b'{"alg":"HS256","typ":"JWT"}')
p = b64(json.dumps({"sub":"agent-42","role":"Admin","exp":int(time.time())+3600}, separators=(",",":")).encode())
d = h + b"." + p
print((d + "." + b64(hmac.new(b"dev-secret", d.encode(), hashlib.sha256).digest())).decode())
PY
)

curl -s localhost:8080/mcp \
  -H 'content-type: application/json' \
  -H "authorization: Bearer $TOKEN" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

In PowerShell, run the server and mint the token from the same shell so both see
the same secret. Pass `-Secret` explicitly rather than relying on the ambient
variable, so the helper works from any terminal:

```powershell
# 1. Start the server with a signing secret (must match -Secret below).
$env:JWT_SECRET = "dev-secret"
cargo run --manifest-path C:\path\to\Gritshield\examples\mcp_server\Cargo.toml

# 2. In another shell, mint a token and call the server.
function New-GritToken(
    [string]$Role,
    [string]$Secret = "dev-secret",
    [int]$TtlSeconds = 3600
) {
    function B64Url([byte[]]$b) {
        [Convert]::ToBase64String($b).TrimEnd('=').Replace('+','-').Replace('/','_')
    }
    $now  = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
    $head = B64Url ([Text.Encoding]::UTF8.GetBytes('{"alg":"HS256","typ":"JWT"}'))
    $body = B64Url ([Text.Encoding]::UTF8.GetBytes(
        (@{ sub = "agent-42"; role = $Role; exp = $now + $TtlSeconds } | ConvertTo-Json -Compress)))
    $data = "$head.$body"
    $mac  = [Security.Cryptography.HMACSHA256]::new([Text.Encoding]::UTF8.GetBytes($Secret))
    "$data.$(B64Url ($mac.ComputeHash([Text.Encoding]::UTF8.GetBytes($data))))"
}

$token = New-GritToken -Role "Operator"          # TTL is in SECONDS
$hdr   = @{ Authorization = "Bearer $token" }

# Operator-only capabilities now show up...
(Invoke-RestMethod -Uri http://localhost:8080/mcp -Method POST `
  -ContentType 'application/json' -Headers $hdr `
  -Body '{"jsonrpc":"2.0","id":1,"method":"tools/list"}').result.tools.name

# ...and calling one succeeds.
(Invoke-RestMethod -Uri http://localhost:8080/mcp -Method POST `
  -ContentType 'application/json' -Headers $hdr `
  -Body '{"jsonrpc":"2.0","id":2,"method":"tools/call",
          "params":{"name":"search_incidents","arguments":{"query":"checkout"}}}'
).result.structuredContent
```

Three things trip people up here:

- **The secret must match, and the server needs it at launch.** `JWT_SECRET` is
  read from the server's process environment on every request, so setting it in
  a shell *after* the server is already running has no effect — restart it. A
  mismatch surfaces as `Signature mismatch!`, and a server started without it
  answers `Bearer authentication is unavailable: JWT_SECRET is not configured`.
- **Give the token a real expiry, in seconds.** An `exp` of `0`, or one already in
  the past, is rejected with `Bearer token rejected: Token expired`.
- **Check the payload, not PowerShell's default rendering.**
  `result.tools` prints as `System.Object[]`; pipe it through
  `| ConvertTo-Json -Depth 10` to actually see the capabilities.

### Roles are not hierarchical by default

`role` is matched exactly, plus any inheritance the application configures.
`Operator` does **not** imply `Admin`, so an `Admin` token is not automatically
allowed to call an `Operator`-gated tool, and vice versa. Either grant the role
the tool asks for, or declare the inheritance on the router — `has_role` walks
the resulting tree for parent roles:

```rust
let router = Router::new()
    .add_middleware(...)
    .add_role_inheritance("Admin", vec!["Operator"]); // Admin now satisfies Operator
```

`MCP_REQUIRE_AUTH=true` additionally rejects anonymous callers with `401`,
which is what you want for anything reachable off-host.

Relevant variables:

| Variable | Default | Meaning |
| --- | --- | --- |
| `MCP_REQUIRE_AUTH` | `false` | Reject anonymous MCP callers outright |
| `MCP_HTTP_PREFIX` | `/mcp` | Mount the transport somewhere else |
| `MCP_ALLOW_STATELESS` | `true` | Allow HTTP requests with no session |
| `MCP_STDIO_ROLE` | `Admin` | Role assumed by the stdio transport |
| `MCP_STDIO_SUBJECT` | `stdio` | Subject recorded in the audit log |

The stdio transport is the easy way to explore as an operator: it assumes the
`Admin` role, so every capability is visible without configuring auth.

## The capability manager

With the `admin` feature (this example enables it), `/admin/mcp` shows every
registered capability, who may use it, whether its kill switch is on, and a
rolling audit log of agent calls:

```text
http://localhost:8080/admin/mcp
```

`/admin/api/mcp` returns the same state as JSON, for scripting and monitoring.

Both sit behind the admin session check, so they answer `401` until you log in
at `/admin/login` — deliberately, since the page can flip a capability off.

Try it: enable `delete_incident` from the page, and the call that returned
`-32001` now runs. Flip it back and it stops.

## The guard order

Every tool call passes through the same fixed sequence, and the sequence is not
configurable:

1. **Authentication** — bearer JWT, admin session, or anonymous
2. **Kill switch** — a disabled tool never reaches your code
3. **RBAC** — `required_role` on the tool, resource or prompt
4. **Schema validation** — arguments checked against `schema`
5. **Your handler**

Your handler is only reached if all four pass, so a tool cannot accidentally
"handle" a request it should have rejected.

## Two conventions worth knowing

**Errors from a handler are not protocol errors.** Returning `Err` produces a
tool result with `isError: true`, which is the MCP convention: the model reads
the message and can correct itself. Reserve real JSON-RPC errors for cases where
retrying cannot help.

**`#[mcp_tool]` handlers are free functions.** Put the capability at module
scope, not on an `impl`. `service = "…"` is metadata for the admin panel; the
handler itself does not need a receiver.

## Next

- The macro error messages are deliberately specific — if a signature is wrong,
  the build fails with the rule it broke rather than a type error.
- Handlers may take the context by value or by reference; both compile.
- `#[mcp_resource]` takes zero or one parameter, `#[mcp_prompt]` takes exactly
  one (the whole payload, typed or `serde_json::Value`).