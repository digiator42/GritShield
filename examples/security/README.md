# Security — GritShield developer guide

A single runnable service covering XSS prevention, session auth with CSRF
protection, rate limiting and IP blacklisting. No database, no admin panel —
the dependency is `default-features = false`, which is also a live check that
a security-focused service doesn't quietly need the rest of the framework.

```
src/main.rs    middleware pipeline, ordering, public paths
src/xss.rs     output encoding vs declarative input cleaning
src/csrf.rs    sessions, anti-forgery tokens, signed cookies
src/abuse.rs   rate limiting, IP blacklisting, client-IP resolution
```

## Run it

```bash
cargo run --manifest-path examples/security/Cargo.toml
# GritShield listening on http://127.0.0.1:8080
```

Requires `JWT_SECRET` to be set in the environment. The session middleware uses
it to sign cookies:

```bash
export JWT_SECRET="dev-secret"
```

No `APP_ENV` needed. If you set it to `production`, session cookies switch to
`Secure` and your browser will stop sending them over plain HTTP.

---

## The pipeline is ordered cheapest-first

`add_middleware` pushes onto a list and the request walks it front to back, so
order is a security property, not a style choice. If any layer rejects, the
optional `on_response` phase walks the same list backwards over the layers that
ran, so the outermost one has the last word on the response:

```text
blacklisted IP?    ──▶ 403   no session touched, no crypto done
over rate limit?   ──▶ 429   counter check only
cross-origin?      ──▶ CORS headers applied
public route?      ──▶ mint a session, then straight to the handler
unauthenticated?   ──▶ 303   redirect to /auth/login
missing/bad CSRF?  ──▶ 403   state-changing request only
──▶ handler
```

Put auth first and every banned IP costs you a cookie HMAC plus a session-store
lookup per request. That is the difference between shedding load and becoming
the bottleneck.

---

## XSS

The rule: **escape the value, not the document.**

```bash
curl "http://127.0.0.1:8080/xss/unsafe?name=%3Cscript%3Ealert(1)%3C%2Fscript%3E"
# <h1>Hello, <script>alert(1)</script></h1>      <- executes

curl "http://127.0.0.1:8080/xss/safe?name=%3Cscript%3Ealert(1)%3C%2Fscript%3E"
# <h1>Hello, &lt;script&gt;alert(1)&lt;/script&gt;</h1>   <- inert, heading intact

curl "http://127.0.0.1:8080/xss/over-escaped?name=Alice"
# &lt;h1&gt;Hello, Alice&lt;/h1&gt;   <- safe, but your markup is now text too
```

`String` and `&'static str` bodies are sent as **trusted HTML**, which is right
for your markup and catastrophic for a query parameter. `SafeHtml` — what
`Sanitizer::encode` returns — is the only body type that is not re-trusted.
`/xss/over-escaped` is the opposite failure: encoding the whole document is
secure and useless, so recognise the symptom (security fine, page broken).

### Declarative cleaning

```bash
curl -X POST http://127.0.0.1:8080/xss/profile \
  -H 'content-type: application/json' \
  -d '{"email":"  USER@ExAmPlE.com ","display_name":"<b>Alice</b>",
       "tagline":"hello%20world","address":{"country":" pt "},"age":31}'
```

```json
{"cleaned":{"address":{"country":"PT"},"age":31,
            "display_name":"&lt;b&gt;Alice&lt;&#x2F;b&gt;",
            "email":"user@example.com","tagline":"hello world"}}
```

`#[derive(GritSanitizer)]` plus `#[clean(...)]` attributes applied by one
`sanitize()` call:

| Attribute | Result above |
|---|---|
| `trim`, `lowercase` | `"  USER@ExAmPlE.com "` → `"user@example.com"` |
| `html_escape` | `<b>Alice</b>` → escaped entities |
| `url_decode` | `hello%20world` → `hello world` |
| `nested`, `trim`, `uppercase` | `" pt "` → `"PT"` through the child struct |
| *(none)* | `age` untouched |

Both layers have a place: encode at the point of output for one-off rendering,
clean the whole payload when you want declarative normalisation rules. Neither
replaces the other.

---

## CSRF and sessions

### The rules, as verified

| Request | Result |
|---|---|
| `POST /auth/session`, anonymous, **no token** | `200`, logged in |
| `GET /auth/me`, anonymous | `303` → `/auth/login` |
| `POST /auth/preferences`, session, **no token** | `403` Anti-Forgery Token Validation Rejected |
| `POST /auth/preferences`, session, valid token | `200` |

Two things fall out of that table.

**1. `enable_csrf` is off unless you turn it on.**
`AuthMiddleware::new_session` sets it to `false`, and the framework docs
(`docs/docs_content/03_security`) describe the guard as automatic in session
mode. In session mode a browser attaches your cookie without being asked, which
is exactly the situation CSRF exists for, so this example sets it explicitly:

```rust
let mut auth = AuthMiddleware::new_session(public_paths, Some("/auth/login"));
auth.enable_csrf = true;
```

**2. `public_paths` waives CSRF as well as authentication.** `STEP 1` in
`src/middleware/auth.rs` returns before the token check is ever reached. So
`/auth/session` needs no token — which is correct, since a login form cannot
require a token the visitor does not have yet — but any *other* state-changing
handler you list as public is unprotected.

Rule of thumb: if a handler changes state belonging to a signed-in user, it must
not be public.

### Walkthrough

```bash
export JWT_SECRET=dev-secret
cargo run --manifest-path examples/security/Cargo.toml

# 1. log in (public route, no token needed)
curl -c jar.txt -X POST http://127.0.0.1:8080/auth/session \
  -d 'username=alice&password=correct+horse'

# 2. fetch this session's token
TOKEN=$(curl -s -b jar.txt -c jar.txt http://127.0.0.1:8080/auth/csrf-token | jq -r .csrf_token)

# 3. change a preference with the token
curl -b jar.txt -X POST http://127.0.0.1:8080/auth/preferences \
  -H "X-CSRF-Token: $TOKEN" -d 'theme=dark'

# 4. drop the token -> 403
curl -i -b jar.txt -X POST http://127.0.0.1:8080/auth/preferences -d 'theme=dark'
```

Credentials are `alice` / `correct horse` and `bob` / `hunter2`.

The token travels either in the `X-CSRF-Token` header or a `csrf_token` form
field. It is stored server-side in the session, which is the whole point: a
token that merely *exists* proves nothing, because an attacker's page can send
any value. It has to match something the attacker cannot read.

### Signed cookies

```bash
curl -b jar.txt http://127.0.0.1:8080/auth/preferences
```

```json
{"signed":"dark",
 "unsigned":"dark.a58979a1cbba7c05cd24d3d2ca2191a82cb19e4942a622d195d6ffa61c8962f5"}
```

`get_signed_cookie` verifies the HMAC and returns `dark`. `get_cookie` returns
the wire value — payload plus signature — untouched. Read anything
security-relevant through the signed accessor.

One subtlety: a cookie set during a request is **not** readable in that same
request (`"read_back": null` above), because reading inspects the incoming
request and the response has not been sent yet. It is available on the next
request.

`Cookie::new` also defaults to `secure: true` and `SameSite::Strict`. A browser
will not store a `Secure` cookie over plain HTTP, so local development needs
the same flip the framework's own session cookie makes:

```rust
let cookie = Cookie::new("theme", theme)
    .set_secure(get_env("APP_ENV", "development") == "production")
    .set_same_site(SameSite::Lax);
```

---

## Rate limiting and blacklisting

The limiter is capped at 100 requests/minute per client, chosen to be easy to
trip. 110 requests:

```
200 : 100
429 : 10
```

```bash
# trip it
for i in $(seq 1 110); do
  curl -s -o /dev/null -w "%{http_code}\n" http://127.0.0.1:8080/abuse/ping
done | sort | uniq -c
```

Buckets are keyed on `resolve_client_ip()`, so one noisy client cannot exhaust
anyone else's budget — confirmed by a second IP still answering `200` afterwards.

The blacklist is literal IPs and covers every route it is installed on,
including admin and MCP endpoints:

```bash
curl -i -H "X-Forwarded-For: 203.0.113.66" http://127.0.0.1:8080/abuse/ping
# 403 Access denied. Your IP address has been blocked.
```

### `X-Forwarded-For` is trusted blindly

`resolve_client_ip()` takes the **leftmost** `X-Forwarded-For` entry and falls
back to the socket address. There is no trusted-proxy allowlist:

```bash
curl -H "X-Forwarded-For: 8.8.8.8, 10.0.0.1" http://127.0.0.1:8080/abuse/whoami
# {"forwarded_for":"8.8.8.8, 10.0.0.1","resolved_ip":"8.8.8.8"}
```

Behind a real edge that is the behaviour you want. Exposed directly to the
internet it is not: a client picks its own header and therefore picks both its
blacklist verdict and its rate-limit bucket. Strip or overwrite the header at
your edge before it reaches the app.

---

## What this example deliberately does not do

- No user store — `authenticate()` accepts two hard-coded pairs.
- No HTTPS — run behind TLS in anything real.
- No `APP_ENV` handling beyond cookie flags.
- Secrets are hard-coded because a guide has to be runnable; a service should
  read them from config or a secret manager.

## See also

- `docs/docs_content/03_security/index.md` — feature overview
- `docs/docs_content/03_security/data_sanitization.md` — sanitizer attributes
- `examples/mcp_server` — RBAC, and bearer-token authentication