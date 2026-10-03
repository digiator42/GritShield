# 🤖 MCP Server - Developer Guide

GritShield ships a native [Model Context Protocol](https://modelcontextprotocol.io) server, so Claude,
Codex, Cursor, or any other MCP client can use your service as a tool surface.

Capabilities are declared on your own types and registered at compile time. There is no hand-written
JSON-RPC, no manual `tools/list` payload, and no router entry to keep in sync — the schema you declare
is the schema the client sees.

---

## Contents

- [Declaring a tool](#declaring-a-tool)
- [Resources](#resources)
- [Prompts](#prompts)
- [Transports](#transports)
- [Discovery is filtered by authorisation](#discovery-is-filtered-by-authorisation)
- [Security model](#security-model)
- [Authentication](#authentication)
- [Environment variables](#environment-variables)
- [The capability manager](#the-capability-manager)
- [Runnable example](#runnable-example)

---

## Declaring a tool

```rust
use gritshield::mcp_tool;
use gritshield::routing::engine::RequestContext;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct LookupArgs {
    user_id: String,
}

#[mcp_tool(
    name = "lookup_user",
    service = "UserService",
    schema = r#"{"type":"object","properties":{"user_id":{"type":"string"}},"required":["user_id"]}"#
)]
async fn lookup_user(
    ctx: &RequestContext,
    args: LookupArgs,
) -> Result<Value, String> {
    // `ctx` carries the authenticated subject, role and audit context.
    Ok(serde_json::json!({ "id": args.user_id, "by": ctx.claims.is_some() }))
}
```

The handler signature is enforced at compile time:

| Parameter | Accepted types |
| --- | --- |
| Context (first) | `RequestContext` or `&RequestContext` |
| Arguments (second) | `serde_json::Value` or any `Deserialize` type |

The order is not a convention. A mis-ordered signature, or a wrong context type, fails the build
instead of silently misbehaving at runtime.

```rust
// Raw arguments: useful when the payload is forwarded as-is.
#[mcp_tool(name = "proxy", service = "Proxy", schema = r#"{"type":"object"}"#)]
async fn proxy(ctx: &RequestContext, args: Value) -> Result<Value, String> {
    Ok(args)
}
```

The `schema` you declare is validated against the arguments **before** your handler runs, so a
malformed call is rejected at the protocol level rather than inside your business logic.

---

## Resources

Resources return their body as a `String`. They take no arguments, or the request context:

```rust
use gritshield::mcp_resource;
use gritshield::routing::engine::RequestContext;

#[mcp_resource(uri = "gritshield://config", mime_type = "application/json")]
async fn read_config(_ctx: &RequestContext) -> Result<String, String> {
    Ok(r#"{"mode":"production"}"#.to_string())
}
```

---

## Prompts

Prompts receive the whole argument payload and return the messages to inject. Exactly one payload
parameter is accepted — raw or typed — and the return type must be `Vec<McpPromptMessage>`.

```rust
use gritshield::mcp_prompt;
use gritshield::mcp::prompt::McpPromptMessage;
use serde::Deserialize;

#[derive(Deserialize)]
struct TriageArgs {
    summary: String,
    #[serde(default)]
    severity: Option<String>,
}

#[mcp_prompt(
    arguments = "summary!,severity?",
    descriptions = "summary=Alert summary;severity=low | medium | high"
)]
async fn triage(args: TriageArgs) -> Result<Vec<McpPromptMessage>, String> {
    Ok(vec![McpPromptMessage::user(format!(
        "Triage this {} alert: {}",
        args.severity.unwrap_or_else(|| "medium".into()),
        args.summary
    ))])
}
```

Required arguments are enforced centrally from the `arguments` declaration, before your handler runs.
A call missing `summary` returns `INVALID_PARAMS` (`-32602`) naming the argument, and your code is
never entered.

The `!` suffix marks an argument required, `?` marks it optional.

---

## Transports

The `mcp` feature is on by default and mounts:

| Method | Path | Purpose |
| --- | --- | --- |
| `GET` | `/mcp/sse` | Server-sent event stream for server-to-client messages |
| `POST` | `/mcp/message?session_id=<id>` | Message delivery for an SSE session |
| `POST` | `/mcp` | Streamable HTTP transport (stateless or session-bound) |
| `DELETE` | `/mcp` | Terminate a streamable session |
| `GET` | `/mcp` | Server info / handshake |

`GET /mcp` returns server info for the handshake.

There is also a **stdio** transport, which is the quickest way to explore as an operator: it assumes
the `Admin` role, so every capability is visible without configuring auth.

Opt out of the whole surface with `default-features = false`.

---

## Discovery is filtered by authorisation

`tools/list`, `resources/list` and `prompts/list` only show what the *calling identity* may actually
use, so a model is never offered a capability it would be denied.

This surprises people the first time: an anonymous HTTP caller sees nothing that declares
`required_role`, so a role-gated tool is missing from the list **and** denied if called directly. That
is the intended behaviour, not a bug. Grant the caller the role, or mint a token carrying it.

---

## Security model

Every call is checked in a fixed order, and the order is not configurable:

1. **Authentication** — bearer JWT (`JWT_SECRET`), an authenticated admin session, or anonymous
   when `MCP_REQUIRE_AUTH` is unset.
2. **Kill switch** — a tool disabled from `/admin/mcp` never reaches your code.
3. **RBAC** — each tool, resource and prompt declares its own `required_role`.
4. **Schema validation** — arguments are validated against the declared JSON Schema.
5. **Your handler.**

Keeping that sequence in one place is the point. Scattering these checks across handlers is how
guardrails get skipped.

Two notes for existing apps:

- `AuthMiddleware` gates `/mcp/**` by default. If you authenticate MCP clients with their own bearer
  tokens, add `"/mcp/**"` to your `public_paths` — MCP runs its own authentication.
- CSRF protection only engages when you enable it *and* run without a JWT handler. Cookie-only
  deployments should keep `"/mcp/**"` out of `public_paths`.

Roles are matched exactly. `Operator` does not imply `Admin` unless you declare the relationship on
the router, and `has_role` walks the resulting tree for parent roles:

```rust
let router = Router::new()
    .add_middleware(...)
    .add_role_inheritance("Admin", vec!["Manager", "Operator", "Auditor"]);
```

---

## Authentication

A verified MCP bearer token becomes the request identity, so `required_role` is checked against the
token's `role` claim even when no HTTP session exists.

An identity the request already carries — a session cookie or framework claims — always wins. An MCP
credential can therefore never elevate a caller who is already authenticated, and a token with no role
claim grants nothing.

Tokens are HS256 over `{ "sub", "role", "exp" }`. Set the secret and restart the server:

```env
JWT_SECRET=dev-secret
```

```bash
curl -s localhost:8080/mcp \
  -H 'content-type: application/json' \
  -H "authorization: Bearer $TOKEN" \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
```

> [!IMPORTANT]
> `JWT_SECRET` is read from the server's process environment on every request. Setting it in another
> shell *after* the server started has no effect — restart the server.

Two failure modes worth recognising:

| Message | Cause |
| --- | --- |
| `JWT_SECRET is not configured` | The server was started without the variable |
| `Signature mismatch!` | The signing secret differs from the server's |
| `Token expired` | `exp` is in the past — remember the TTL is in **seconds** |
| `Role 'X' is required…` | Valid token, wrong `role` claim |

---

## Environment variables

| Variable | Default | Meaning |
| --- | --- | --- |
| `MCP_REQUIRE_AUTH` | `false` | Reject anonymous MCP callers |
| `MCP_HTTP_PREFIX` | `/mcp` | Mount the transport somewhere else |
| `MCP_ALLOW_STATELESS` | `true` | Allow HTTP requests with no session |
| `MCP_STDIO_ROLE` | `Admin` | Role assumed by the stdio transport |
| `MCP_STDIO_SUBJECT` | `stdio` | Subject recorded in the audit log |

`MCP_REQUIRE_AUTH` defaults to permissive so the server is usable immediately, but every capability
still enforces its own `required_role` — an anonymous caller can only reach what is explicitly
unrestricted. Set it to `true` for anything reachable off-host.

---

## The capability manager

With the `admin` feature enabled, `/admin/mcp` lists every registered tool, resource and prompt with
its required role, kill switch, live counters, last invocation and last error, alongside a rolling
audit log of agent calls.

```text
http://localhost:8080/admin/mcp
```

`/admin/api/mcp` returns the same state as JSON, for scripting and monitoring.

Both sit behind the admin session check and answer `401` until you log in at `/admin/login` —
deliberately, since the page can flip a capability off.

> [!NOTE]
> The admin pages read entities from the database, so mount the pool on the router or they will fail
> with a missing database connection:
>
> ```rust
> let router = Router::new().mount_db(shared_db.clone());
> ```

---

## Runnable example

[`examples/mcp_server`](https://github.com/digiator42/GritShield/tree/main/examples/mcp_server) is a
fully commented walkthrough — RBAC across several roles, a disabled destructive tool, resources, typed
and raw prompts, client configuration, and the admin panel.

```bash
cargo run --manifest-path examples/mcp_server/Cargo.toml
```

Read `src/tools.rs`, `src/resources.rs` and `src/prompts.rs` in order; they are the guide.