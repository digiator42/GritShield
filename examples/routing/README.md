# Routing — GritShield developer guide

Route declaration, matching, custom middleware and error handling.

```
src/main.rs       pipeline order, manual registration, route tree dump
src/basics.rs     controllers, path params, priority, verbs, query strings
src/middleware.rs writing the Middleware trait and AfterRequestHook
src/errors.rs     custom 404 / 405 pages with #[catch]
```

## Run it

```bash
cargo run --manifest-path examples/routing/Cargo.toml
# GritShield listening on http://127.0.0.1:8081
```

Port **8081**, not 8080 — the security guide uses 8080 and you will want both
running side by side.

On startup the example prints the route trie. It is worth reading once, because
most routing questions are answered by looking at the tree:

```
- [Node] Methods: ["GET"]
  └─ Segment: 'api'
      ├─ Segment: 'items'
      │   Methods: ["GET", "POST"]
      │     └─ Segment: ':id'
      │         Methods: ["PATCH", "PUT", "DELETE"]
      └─ Segment: 'users'
          ├─ Segment: ':id'
          │   Methods: ["GET"]
          │     └─ Segment: 'posts'
          │         └─ Segment: ':post_id'
          └─ Segment: 'profile'
              Methods: ["GET"]
```

That single dump shows the two rules that matter: `items/:id` holds three verbs
under one node (so a wrong verb is a `405`, not a `404`), and `users` holds both
`:id` and `profile` as siblings (so exact beats parameter).

---

## Three rules

**1. `:name` captures exactly one segment.** There is no multi-segment wildcard.

```bash
curl -i http://127.0.0.1:8081/static/logo.png     # 200, *path = "logo.png"
curl -i http://127.0.0.1:8081/static/css/app.css  # 404, two segments
```

`/static/:*path` reads like a catch-all and is not one — `*path` is just a
parameter name. `router.debug_dump_tree()` shows it stored as a literal
`:*path` child. To serve a directory, register the routes you need or resolve
the subtree in a handler that owns it.

**2. Exact beats parameter, and declaration order is irrelevant.**

```bash
curl http://127.0.0.1:8081/api/users/profile
# {"matched":"exact route, not the :id parameter"}

curl http://127.0.0.1:8081/api/users/anything
# {"id":"anything", ...}
```

`/api/users/profile` is registered *after* `/api/users/:id` and still wins,
because the matcher tries an exact child before falling back to `:param`.

**3. Right path, wrong verb is `405`.**

```bash
curl -i http://127.0.0.1:8081/api/items/7
# HTTP/1.1 405 Method Not Allowed
# Allow: PUT, PATCH, DELETE
# {"error":"wrong verb","method":"GET","path":"/api/items/7"}

curl -i -X PUT http://127.0.0.1:8081/api/items/7    # 200
curl -i http://127.0.0.1:8081/api/items             # 200
curl -i -X POST http://127.0.0.1:8081/api/items -d "name=bolt"   # 201
```

## Controllers

`#[controller("/api")]` is **string concatenation, not path joining**:

```rust
#[controller("/api")]
impl ApiController {
    #[get("/ping")] async fn ping() { /* -> /api/ping  */ }
    #[get("ping")]  async fn oops() { /* -> /aping   */ }
}
```

The leading slash on the sub-path is not optional.

A handler does not have to live in a controller block — `home()` at `/` and
`wildcard_trap()` at `/static/:*path` in `basics.rs` are both bare functions,
mounted at the root.

Nothing is registered by hand. `Router::new()` scans the `inventory` registry
that the macros write into, which is the same discovery mechanism the DI
example is about.

## Manual registration

`add_route` is a real option, and it is the one API that breaks the builder
chain — it takes `&mut self` and returns nothing:

```rust
let mut router = Router::new().add_middleware(..).add_after_hook(..);
router.add_route(HttpMethod::GET, "/manual", |_ctx| async { .. }, None);
```

```bash
curl http://127.0.0.1:8081/manual
# {"registered":"by hand"}
```

The fourth argument is the role required to reach the route; `None` means
anyone. Same mechanism as `required_role = "admin"` on a route macro — see
`examples/rbac_caps`.

Note `HttpMethod` variants are uppercase (`HttpMethod::GET`), and the type
derives `Debug` but **not** `Serialize` or `Display`. Putting
`ctx.req.method` straight into a `json!` macro does not compile; use
`format!("{:?}", ..)`.

---

## Middleware

The trait is `async` (`#[async_trait]` on every impl) and has two phases:

```rust
#[async_trait]
pub trait Middleware: Send + Sync {
    async fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult;
    async fn on_response(&self, ctx: &RequestContext, res: &mut Response) {}
}

enum MiddlewareResult {
    Next(Option<MiddlewareState>),  // continue
    Error(Response),                // stop, this is the client's answer
}
```

`Next(None)` is the normal case, and passing `None` matters: it is what stops
your middleware clearing a session an earlier one established.

Registration order is execution order, and every layer can reject — so the cheap
and the broad go first, for the same reason as in the security guide.
`on_response` is optional and unwinds the same list **backwards**, for the
layers that actually ran.

### Rejecting

`ApiKeyMiddleware` guards the `/secure` subtree. It is configured with the
prefix it **guards** rather than the one it exempts, which keeps the rest of the
guide reachable without a header on every call:

```bash
curl -i http://127.0.0.1:8081/secure/report
# 401 {"error":"missing or invalid X-Api-Key"}

curl -i -H "X-Api-Key: dev-key" http://127.0.0.1:8081/secure/report
# 200 {"secret":"only rendered because the middleware let this through", ...}
```

An API key in a header needs no CSRF token, which is exactly why it should not
be the only thing in front of a cookie-authenticated session.

### Passing data to handlers

`MiddlewareState` carries `session`, `claims` and `session_was_stale` and nothing
else — there is no arbitrary-payload channel. Annotate the `RequestContext`
instead:

```rust
ctx.headers.insert("x-request-id".to_string(), vec![format!("req-{n:06}")]);
```

### Response headers: who owns the value

There are two ways to set a header, and picking the wrong one is either a no-op
or a leak.

**Handler owns it -> put it on the `Response`.**

```rust
Response::json(HttpStatus::Ok, &body).with_header("X-Request-Id", request_id)
```

**Middleware owns it before the handler runs -> put it on `ctx.headers`.** The
server builds the response *after* your handler returns, and promotes the names
middleware added to real response headers afterwards
(`Response::merge_middleware_headers`) — on success, on 404/405, on a
rejection, on a panic.

**Middleware owns it after the response exists -> use `on_response`.** It hands
you the actual `&mut Response`, runs in reverse registration order over the
layers that ran, and executes before the lifecycle log, the after-hooks and the
telemetry counters — so a status it rewrites is what gets logged.

The promotion is deliberately narrow:

```text
client sent: Cookie, Authorization, Host, User-Agent, ...
  --> never promoted
middleware added: x-request-id
  --> promoted, unless the handler already set a matching name
```

That first line is a security property, not an implementation detail. Send
credentials in and they are not echoed back out:

```bash
curl -i -H "Cookie: GSESSION_ID=secret123" \
        -H "Authorization: Bearer topsecrettoken" \
        http://127.0.0.1:8081/api/ping
```

```
HTTP/1.1 200 OK
X-Content-Type-Options: nosniff
X-Request-Id: req-000005
Content-Type: application/json
```

No `Cookie`, no `Authorization` -- otherwise any proxy or CDN that caches a
response would be holding your session id or bearer token.

Two details follow from doing this properly. Name matching is case-insensitive,
so a handler's `X-Request-Id` and a middleware's `x-request-id` are the same
header and only one goes out:

```bash
curl -i http://127.0.0.1:8081/mw/echo-request-id
# X-Request-Id: req-000007        <- once, not req-000007 req-000007
```

And the collision check runs per *name*, before the values are copied, so a
multi-valued middleware header forwards all of its values rather than only the
first.

The practical rule: if a handler decides the value, use the `Response`. If
middleware decides it *before* the handler, use `ctx.headers`; if it needs the
response itself to decide (a status override, a timing header), use
`on_response`. Never treat `ctx.headers` as a general response-header bag --
it starts as a copy of the request, and it is no place to put anything
sensitive.

### After-request hooks

`AfterRequestHook` is `async` (`#[async_trait]`) and runs once status and
duration are known — the information a before-hook does not have. If you also
need to *modify* the response, that is `Middleware::on_response`; hooks only
observe:

```
AUDIT GET /api/ping -> 200 in 0ms
AUDIT GET /secure/report -> 401 in 0ms
```

Failures here cannot affect the client, so keep it that way and do not block
the reactor on I/O.

---

## Custom error pages

Without a `#[catch]`, an unmatched path gets `<h1>404 Not Found</h1>`. With one:

```bash
curl -i http://127.0.0.1:8081/no-such-route
# HTTP/1.1 404, application/json
# {"error":"no route matched","hint":"GET / lists the routes this guide registers", ...}
```

Handlers are keyed by status in a registry, so the `404` and `405` handlers
coexist. Two constraints on the function: `async`, and it must take a
`RequestContext`.

### Two obstacles, and one API question

**The attribute needs a raw identifier.** `catch` is a reserved keyword in Rust
2021, so `use gritshield::catch;` does not parse and the prelude does not
re-export it:

```rust
use gritshield::r#catch;

#[r#catch(status = 404)]
pub async fn not_found(ctx: RequestContext) -> Response { .. }
```

**Your crate needs `ctor` as a direct dependency.** The expansion emits
`#[::gritshield::startup::ctor(unsafe)]`, and a `#[ctor]` attribute resolves
against the crate being compiled. Without `ctor = "1.0"` in `Cargo.toml` you get:

```
error[E0433]: cannot find `ctor` in the crate root
```

which points nowhere near the cause. That is why this example's `Cargo.toml`
carries a dependency it otherwise would not need.

**Which `HttpStatus` for 405?** `Response::json` takes an `HttpStatus`, not a
number, so a `#[catch(status = 405)]` handler needs a matching variant —
`HttpStatus::MethodNotAllowed`. Reaching for a number instead is not an option:
`Response::new` takes a `u16` but hardcodes `Content-Type: text/html`, and
appending a second `Content-Type` leaves the wrong one first in the list.

Note that the router already produces 405 on its own. A matched path with the
wrong verb resolves to `RoutingResult::MethodNotAllowed` before any handler
runs; registering a `#[catch]` handler only replaces the body.

```bash
curl -i http://127.0.0.1:8081/api/items/7
# HTTP/1.1 405 Method Not Allowed
# Allow: PUT, PATCH, DELETE
# Content-Type: application/json
# {"error":"wrong verb","method":"GET","path":"/api/items/7","allowed":[...]}
```

That `Allow` header is now the handler's job. A `#[catch]` handler replaces the
response wholesale, and RFC 9110 requires `Allow` on a 405.

---

## Not in the prelude

`gritshield::prelude::*` does not export `HttpStatus`, `catch`, `Middleware`,
`MiddlewareResult` or `AfterRequestHook`. This example imports each from its
module, which is also how you find them in the source:

```rust
use gritshield::http::response::HttpStatus;
use gritshield::middleware::{AfterRequestHook, Middleware, MiddlewareResult};
use serde_json::json;
```

## See also

- `docs/docs_content/04_routing/` — routing, middleware, dynamic routes
- `examples/security` — middleware ordering and the pipeline rationale
- `examples/rbac_caps` — `required_role` on routes
