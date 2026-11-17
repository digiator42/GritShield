# Security Filter Chains

`SecurityFilterChain` is GritShield's Spring Security–style authorization model.
Instead of one global `AuthMiddleware` deciding every route, you declare one or
more chains: each chain matches a set of requests, runs its filters in order,
then applies authorization rules to what survived.

- **Opt-in** — a request matched by no chain is untouched.
- **Composable** — filters are small and reusable (`AuthenticationFilter`,
  `CsrfFilter`, or any type implementing `SecurityFilter`).
- **Secure by default** — a chain with rules denies anything the rules did not
  explicitly cover.
- **Ordered** — declarative chains run by `order`; manually added chains follow
  in insertion order. The first matching chain wins.

A runnable version with JWT, session, role, method and CSRF rules lives in
`examples/security_filter_chain`.

## Quick start

```rust
use gritshield::prelude::*;

let router = Router::new()
    .route(("/api/me", HttpMethod::GET, me_handler))
    .add_security_chain(
        SecurityFilterChain::builder()
            .name("api")
            .matching("/api/**")
            .jwt_auth(JwtHandler::new(&secret))
            .authorize_public("/api/public/**")
            .authorize_role("Admin", "/api/admin/**")
            .any_request(AccessRule::authenticated())
            .build(),
    );
```

With `Shield`, attach the chain the same way:

```rust
Shield::new()
    .add_security_chain(chain)
    .ignite()
    .await;
```

## Declarative chains

Instead of wiring every chain through the router, annotate builders with
`#[security_chain]`. `Router::new()` discovers every annotated chain in the
binary, sorts them by `order` (lower first, then by `name`), and installs them
before any chain added with `.add_security_chain(...)`.

```rust
use gritshield::prelude::*;

#[security_chain(name = "api", order = 10)]
fn api() -> SecurityFilterChain {
    SecurityFilterChain::builder()
        .matching("/api/**")
        .jwt_auth(JwtHandler::new(&secret))
        .any_request(AccessRule::authenticated())
        .build()
}

struct AdminChain;

#[security_chain(name = "admin", order = 5)]
impl AdminChain {
    fn build() -> SecurityFilterChain {
        SecurityFilterChain::builder()
            .matching("/admin/**")
            .session_auth()
            .any_request(AccessRule::has_role("Admin"))
            .build()
    }
}
```

- The function or associated function must be synchronous, take no arguments,
  and return `SecurityFilterChain`.
- An impl block must be inherent and may contain only that one associated
  function.
- `name` defaults to the function name (or the type name for the impl form) and
  becomes the chain's diagnostic name.
- `order` defaults to `0`; use it instead of editing the router when adding a
  chain to a larger app.
- Each integration-test binary gets its own registrations, so chains declared in
  test files stay isolated.
- `Shield::add_security_chain` still works for chains built from runtime values;
  declarative chains always come first.

The example (`examples/security_filter_chain`) uses both forms.

## Where chains run

```
route match → global middleware → security chain → route role check → handler → response
```

- Global middleware runs **first**, so rate limiting, IP blacklists, logging and
  session setup still apply before authorization.
- Security chains run **before** handler dispatch, so they also govern
  WebSocket upgrades.
- The response phase unwinds in reverse: security filter `on_response` runs
  before middleware `on_response`, because the security stage is innermost.

Requests that match no chain never enter a filter and behave exactly as before.

## Matching

`matching` accepts an Ant-style path pattern:

| Pattern | Matches |
|---------|---------|
| `/api/**` | every path under `/api/` (including `/api`) |
| `/api/*` | one segment under `/api/` |
| `/users/:id` | `/users/42` (the segment is captured as `id`) |
| `/files/**` | recursive suffix |

For method, header or predicate matching use `matching_request` with a
`RequestMatcher`:

```rust
SecurityFilterChain::builder()
    .matching_request(RequestMatcher::path_and_method("/uploads", HttpMethod::POST))
    // ...
```

Chains are evaluated in the order they are installed: declarative chains sorted
by `order` (then `name`), followed by `.add_security_chain(...)` calls. Register
narrow chains before broad ones:

```rust
#[security_chain(order = 10)] // /admin/**
fn admin() -> SecurityFilterChain { /* ... */ }

#[security_chain(order = 20)] // /api/**
fn api() -> SecurityFilterChain { /* ... */ }

#[security_chain(order = 30)] // /**
fn web() -> SecurityFilterChain { /* ... */ }
```

## Filters

Filters run front to back. Each returns a `FilterResult`:

| Result | Effect |
|--------|--------|
| `Continue` | run the next filter |
| `Respond(Response)` | stop with this response |
| `Unauthenticated` | stop with the chain's entry point |
| `AccessDenied` | stop with the chain's access-denied handler |

Built-in filters:

```rust
// Cookie sessions: signed GSESSION_ID, CSRF token seeded on creation
.session_auth()

// Bearer tokens: signature and expiry checked; invalid tokens are rejected,
// even on paths that are otherwise public
.jwt_auth(JwtHandler::new(&secret))

// Session first, bearer token as a fallback
.hybrid_auth(JwtHandler::new(&secret))

// Double-submit CSRF for POST/PUT/PATCH/DELETE
.csrf()
.csrf_with(|csrf| csrf.ignore_path("/auth/login"))
```

`AuthenticationFilter` only establishes **who** the caller is
(`ctx.session` / `ctx.claims`). It never decides access — that belongs to the
rules.

Any existing middleware can be used as a filter through the blanket
`SecurityFilter` implementation:

```rust
.add_security_chain(
    SecurityFilterChain::builder()
        .matching("/internal/**")
        .filter(RateLimitMiddleware { limiter })
        .filter(my_audit_filter)
        .any_request(AccessRule::authenticated())
        .build(),
)
```

## Authorization rules

Rules are checked in order; the **first matching rule wins**.

```rust
SecurityFilterChain::builder()
    .matching("/api/**")
    .session_auth()
    .authorize_public("/api/health")
    .authorize_authenticated("/api/profile/**")
    .authorize_role("Admin", "/api/admin/**")
    .authorize(RequestMatcher::path_and_method("/api/items", HttpMethod::POST),
               AccessRule::has_any_role(&["Admin", "Editor"]))
    .any_request(AccessRule::deny_all())
    .build()
```

Available rules:

| Rule | Grants when |
|------|-------------|
| `AccessRule::permit_all()` | always |
| `AccessRule::deny_all()` | never |
| `AccessRule::authenticated()` | a session user or verified JWT claims exist |
| `AccessRule::has_role("Admin")` | the principal holds the role (hierarchy-aware) |
| `AccessRule::has_any_role(&[...])` | the principal holds at least one |
| `AccessRule::has_all_roles(&[...])` | the principal holds every one |
| `AccessRule::custom(f)` | your predicate returns `true` |

**Fail-closed:** if a chain has at least one rule and none of them match the
request, the request is denied. A chain with no rules at all is filters-only and
permits the request after its filters run. Use `any_request(...)` to make the
fallback explicit.

## Entry points and access-denied handlers

An `AuthenticationEntryPoint` answers "no usable identity"; an
`AccessDeniedHandler` answers "identified, but not allowed".

```rust
SecurityFilterChain::builder()
    // JSON 401 for an API
    .entry_point(AuthenticationEntryPoint::Bearer)
    // or send browsers to the login page
    .entry_point(AuthenticationEntryPoint::redirect("/login"))
    .access_denied(AccessDeniedHandler::Forbidden)
    .build()
```

Defaults are a JSON `401 Unauthorized` and a JSON `403 Forbidden`. Both support
`custom(|ctx| Response)` if you need a product-specific body.

## Writing a filter

```rust
#[async_trait]
impl SecurityFilter for TenantFilter {
    fn name(&self) -> &'static str { "tenant" }

    async fn on_request(&self, ctx: &mut RequestContext) -> FilterResult {
        match ctx.header("x-tenant") {
            Some(id) if self.known(id) => {
                ctx.set_session_data("tenant", id);
                FilterResult::Continue
            }
            _ => FilterResult::AccessDenied,
        }
    }

    async fn on_response(&self, _ctx: &RequestContext, res: &mut Response) {
        res.headers.push(("vary".into(), "x-tenant".into()));
    }
}
```

`on_response` runs only for filters whose `on_request` executed, in reverse
order. A filter added after a short-circuit never sees either phase.

## Layering with `AuthMiddleware`

Chains and `AuthMiddleware` can coexist. Keep `AuthMiddleware` for legacy
routes and introduce chains incrementally; a chain only affects the paths its
matcher covers. When both run, global middleware finishes first, so the chain
sees whatever session or claims `AuthMiddleware` established.
