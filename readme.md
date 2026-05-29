## What is GritShield?

GritShield is an **async-first, security-hardened** web framework for Rust that eliminates the majority of OWASP Top 10 vulnerabilities by design.

## Key Features

- **Native Default Asynchrony** – Built entirely from the ground up on non-blocking `async/await` design patterns, leveraging a multi-threaded Tokio runtime context to sustain massive concurrent execution throughput without thread starvation.
- **XSS-Safe Templating** – Untrusted data cannot reach HTML without explicit sanitisation.
- **CSRF Protection** – Automatic token validation for state-changing requests.
- **Signed Cookies** – Cryptographic HMAC-SHA256 signatures prevent client-side cookie tampering.
- **Session Management** – Thread-safe, in-memory store protected by asynchronous synchronization locks with automatic expiration hooks.
- **JWT Support** – Stateless authentication via securely managed HS256 tokens.
- **Rate Limiting** – Sliding-window rate tracking per IP address, backed by low-overhead atomic counters.
- **IP Blacklisting** – Instantly drops connection sockets for malicious clients directly at the early middleware layer.
- **File-Based Routing** – Auto-discovery of structural endpoint handlers mapped straight to your local filesystem hierarchy.

## Quick Navigation

- [Getting Started](/docs/getting-started) - First steps with GritShield
- [Architecture Overview](/docs/architecture/overview) - Understand the kernel design
- [Security Guide](/docs/security) - Learn about security features
- [Routing Guide](/docs/routing) - Master the routing system

# GritShield Documentation

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
gritshield = "0.1.0"
tokio = { version = "1", features = ["full"] }
```

---

## Quick Start – Hello World

Create `src/main.rs`:

```rust
use gritshield::prelude::*;

#[get("/")]
async fn hello(_: RequestContext) -> &'static str {
    "Hello, GritShield!"
}

#[tokio::main]
async fn main() {
    let router = Router::new().mount_logger();

    run_server("127.0.0.1", "8080", router, false).await;
}
```

Run with `cargo run` and open `http://localhost:8080`.

---

## Core Concepts

## 1. Request Context

There is no more importing this::that or constantly checking the docs just to find out where a feature lives.

Every handler receives a `RequestContext` that holds the HTTP request, dynamic parameters, session, cookies, parsed form data, JSON body, and security helpers all in one place.

```rust
async fn profile(ctx: RequestContext) -> String {
    let user_id = ctx.params.get("id").unwrap().as_str();
    let session_user = ctx.get_session_data("user_id").unwrap_or_default();

    format!("Viewing profile of {}", user_id)
}
```

---

## 2. Routing

- ### a) Attribute Macros

  ```rust
  #[get("/users/:id")]
  async fn get_user(ctx: RequestContext) -> Response {
      let id = ctx.params.get("id").unwrap().as_str();
      Response::new(Sanitizer::trust(format!("User {}", id)))
  }

  #[post("/users")]
  async fn create_user(ctx: RequestContext) -> Result<Response, ShieldError> {
      let new_user: CreateUserDto = ctx.json()?;
      // ...
      Ok(Response::redirect(303, "/users"))
  }
  ```

  Supported methods:
  - `#[get]`
  - `#[post]`
  - `#[put]`
  - `#[patch]`
  - `#[delete]`

- ### b) Manual Registration

  ```rust
  router.add_route(HttpMethod::GET, "/health", |_: RequestContext| async move { "OK" });

  ```

- ### c) File‑Based Routing (Next.js style)

  Place your handlers in `src/pages/`:
  - `src/pages/index.rs` → route `/`
  - `src/pages/users/[id].rs` → route `/users/:id`
  - `src/pages/api/[..path].rs` → route `/api/**` (catch‑all)

  Inside the file, use the `register_page!` macro:

  ```rust
  use gritshield::prelude::*;

  register_page!(HttpMethod::GET, |_| async { "Hello from file route" });
  ```

  GritShield automatically discovers `.rs` files under `src/pages` and mounts them.

---

## 3. Middleware Stack

- Middleware implements the `Middleware` trait. They run in the order they are added.

  ```rust
  pub trait Middleware: Send + Sync {
      fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult;
  }
  ```

  ### Built‑in middleware
  - `LoggerMiddleware` – logs method, path, status, duration, authentication state.
  - `RateLimitMiddleware` – sliding‑window rate limiting.
  - `IPBlacklistMiddleware` – blocks requests from configured IP addresses.
  - `AuthMiddleware` – session + JWT + CSRF gatekeeper.

  ### Adding custom middleware

  ```rust
  struct MyMiddleware;

  impl Middleware for MyMiddleware {
      fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
          // modify context or reject
          MiddlewareResult::Next(None)
      }
  }

  router = router.add_middleware(MyMiddleware);
  ```

---

## 4. Responses

Handlers can return any type that implements `IntoResponse`:

- `Response` – full control over status, headers, cookies.
- `&'static str / String` – automatically wrapped as HTML.
- `Result<T, ShieldError>` – maps errors to a safe error page.
- `SafeHtml` – pre‑sanitised HTML content.

### Constructors

```rust
Response::new(200, Sanitizer::trust("<h1>Welcome</h1>"))
Response::new(200, );
Response::redirect(303, "/login");
Response::json(200, &my_struct);
Response::static_file("static/style.css");
```

---

## 5. Static Files

Place files in the `static/` folder.

Serve them with:

```rust
Response::static_file("static/logo.png")
```

Serve static files in one go

```rust
#[get("/static/:*path")]
async fn static_assets(ctx: RequestContext) -> Response {
    let path = ctx.params.get("*path").unwrap().as_str();

    Response::static_file(&format!("/static/{}", path).as_str())
}
```

Directory traversal is prevented automatically.

---

## AuthMiddleware – Session vs JWT

By using AuthMiddleware you have a full authentication system that exposes only `/login`&`/register` routes, set signed `hmac cookies`, generate a new `CSRF` token immediately, `logs out` user automatically and redirects unauthenticated users to `/login`

### Session mode (default)

```rust
let auth = AuthMiddleware::new_session(
    vec!["/login".to_string(), "/register".to_string()],
    Some("/login")
);

router = router.add_middleware(auth);
```

Features:

- Creates a signed `GSESSION_ID` cookie.
- Stores user data in an in‑memory `SessionStore`.
- Use `ctx.login_user_id("123")` to authenticate.
- `ctx.is_user_authenticated()` checks login state (expects to set `user_id` in session once user login).

### JWT stateless mode

```rust
let jwt_handler = JwtHandler::new(&std::env::var("JWT_SECRET").unwrap());

let auth = AuthMiddleware::new_jwt(
    jwt_handler,
    vec!["/public".into()],
    None
);
```

Features:

- Expects `Authorization: Bearer <token>`.
- Stateless authentication.
- Claims available through `ctx.claims`.

---

## CSRF Protection

Enabled automatically in session mode.

HTML forms must include csrf_token value:

Retrieve the token:

```rust
let token = ctx.get_csrf_token();

// using maud
render!(ctx, "title", html! {
    input type="hidden" id="global-csrf-token" value=(csrf_token);
})
```

---

## Cookies – Signed & Secure

```rust
// Read a signed cookie
if let Some(val) = ctx.get_signed_cookie("user_pref") {
    // ...
}

// Set a signed cookie
let cookie = Cookie::new("pref", "dark_mode")
    .set_secure(cfg!(production))
    .set_same_site(SameSite::Lax);

ctx.set_signed_cookie(cookie);

// Delete a cookie
ctx.remove_cookie("pref");
```

Unsigned cookies are also available:

```rust
ctx.get_cookie()
ctx.set_cookie()
```

---

## XSS Prevention

All user input arrives as `UntrustedString`.

To display safely:

```rust
let safe_html = Sanitizer::encode(untrusted_string);
```

To return trusted HTML:

```rust
Sanitizer::trust("...")
```

Only use for strings you fully control.

---

## Rate Limiting

```rust
let limiter = RateLimiter::new(50, Duration::from_secs(60));

let rate_middleware = RateLimitMiddleware { limiter };

router = router.add_middleware(rate_middleware);
```

---

## IP Blacklisting

```rust
let blacklist = IPBlacklistMiddleware::new(vec![
    "192.168.1.100",
    "10.0.0.5"
]);

router = router.add_middleware(blacklist);
```

---

# Database Integration

GritShield integrates with SeaORM.

```rust
let db = sea_orm::Database::connect(
    "postgres://user:pass@localhost/db"
).await.unwrap();

let router = Router::new().mound_db(Arc::new(db));
```

Access the connection inside handlers:

```rust
async fn list_users(ctx: RequestContext) -> String {
    let db = ctx.db.as_ref().unwrap();

    // use db
}
```

---

# Request Data Parsing

Supported formats:

- JSON → `ctx.json::<T>()`
- Form URL‑encoded → `ctx.form.fields`
- Multipart uploads → `ctx.form.files`
- Query parameters → `ctx.query`
- Raw body → `ctx.raw_body`

### Example file upload

```rust
#[post("/upload")]
async fn upload(ctx: RequestContext) -> String {
    if let Some(file) = ctx.form.files.get("avatar") {
        std::fs::write(
            format!("uploads/{}", file.filename),
            &file.data
        ).unwrap();

        "Uploaded!"
    } else {
        "No file".into()
    }
}
```

---

# Templating

Place templates in the `templates/` folder.

```rust
use gritshield::html::TemplateEngine;

// Precompile templates, this should be used once, then get template will be cashed
TemplateEngine::precompile_all("templates").unwrap();

// Render template
let html = TemplateEngine::get("home.html");

Response::new(200, Sanitizer::trust(&html))
```

You can integrate with templating crates like Handlebars.

---

# Configuration & Environment

Environment variables are loaded from `.env` or the system.

### Important variables

- `APP_ENV`
- `JWT_SECRET`

### Access variables

```rust
let db_url = gritshield::core::env::get_env(
    "DATABASE_URL",
    "sqlite::memory:"
);
```

---

# Telemetry & Logging

Enable request logging:

```rust
router = Router::new().mount_logger();
```

Example log output:

```text
🗲  [200] GET /dashboard ➔  Size: 2.34 KB | Time: 12ms | Auth: 🍪 Session ID: a1b2c3d4
```

Supports custom telemetry hooks and metrics collection.

---

# Hot Reload (Development)

```rust
run_server("127.0.0.1", "8080", router, true).await;
```

Automatically rebuilds and reloads on source changes.

---

# Error Handling

The framework catches errors globally.

### Development mode

- Detailed technical information shown.

### Production mode

- Generic user‑safe error page.
- Errors logged server‑side.

### Custom error handler

```rust
fn custom_error_handler(
    ctx: RequestContext,
    err: ShieldError
) -> BoxFuture<'static, Response> {

    Box::pin(async move {
        Response::new(500, Sanitizer::trust("Oops"))
    })
}

router.global_error_handler.handler = Some(custom_error_handler);
```

---

# Deployment

### Recommended production setup

- Set `APP_ENV=production`
- Use strong `JWT_SECRET`
- Use reverse proxy (Nginx/Caddy)
- Compile with `--release`

### Example Dockerfile

```dockerfile
FROM rust:1.70 as builder

WORKDIR /app

COPY . .

RUN cargo build --release

FROM debian:bookworm-slim

COPY --from=builder /app/target/release/myapp /usr/local/bin/

ENV APP_ENV=production

CMD ["myapp"]
```

---

# Testing Helpers

```rust
#[tokio::test]
async fn test_handler() {
    let ctx = RequestContext::new();

    let response = my_handler(ctx).await;

    assert_eq!(response.status, 200);
}
```

---

# API Reference (Selected)

| Type / Function                             | Description                         |
| ------------------------------------------- | ----------------------------------- |
| `Router::new()`                             | Creates empty router.               |
| `router.add_route(method, path, handler)`   | Manual route registration.          |
| `router.add_middleware(M)`                  | Appends middleware to the pipeline. |
| `RequestContext::get_signed_cookie(name)`   | Reads signed cookie.                |
| `RequestContext::set_signed_cookie(cookie)` | Writes signed cookie.               |
| `RequestContext::remove_cookie(name)`       | Deletes cookie.                     |
| `RequestContext::login_user_id(id)`         | Sets authenticated user ID.         |
| `RequestContext::get_csrf_token()`          | Returns CSRF token.                 |
| `Response::redirect(status, location)`      | Redirect response.                  |
| `Response::json(status, &data)`             | Serialises JSON response.           |
| `Sanitizer::encode(UntrustedString)`        | Escapes HTML safely.                |
| `Sanitizer::trust(str)`                     | Creates trusted HTML.               |
| `RateLimiter::new(max, window)`             | Creates rate limiter.               |
| `JwtHandler::sign(claims)`                  | Signs JWT token.                    |
| `JwtHandler::verify(token)`                 | Verifies JWT token.                 |
| `TemplateEngine::get(name)`                 | Returns template content.           |

---

# Contributing

GritShield is built with security as the highest priority.

Contributions, bug reports, and security advisories are welcome.

Please open an issue before submitting a PR.

---

# License

Apache‑2.0.
