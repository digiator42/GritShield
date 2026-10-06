
Middleware runs around handlers: `execute` sees the request before the handler runs, `on_response` sees the response after it — and both run on every response, rejections and 404s included.


## Built-in Middleware

**LoggerMiddleware**

---

Logs each request with method, path, status, duration, and auth info:

```rust
router = router.add_middleware(LoggerMiddleware);
```

> Output: 🗲 [200] GET /dashboard ➔ Size: 2.34 KB | Time: 12ms | Auth: 🍪 Session ID: a1b2c3d4

**RateLimitMiddleware**

---

Prevents abuse with per-IP rate limiting:

```rust
let limiter = RateLimiter::new(100, Duration::from_secs(60));
router = router.add_middleware(RateLimitMiddleware { limiter });
```

**IPBlacklistMiddleware**

---

Blocks specific IP addresses:

```rust
let blacklist = IPBlacklistMiddleware::new(vec!["192.168.1.100", "10.0.0.5"]);
router = router.add_middleware(blacklist);
```

**AuthMiddleware**

---

Handles authentication, sessions, JWT, and CSRF:

```rust
// Session mode
let auth = AuthMiddleware::new_session(
    vec!["/login".to_string(), "/register".to_string()],
    Some("/login")
);

// JWT mode
let jwt = JwtHandler::new(&secret);
let auth = AuthMiddleware::new_jwt(jwt, vec!["/public".into()], None);

router = router.add_middleware(auth);
```

## Custom Middleware

### Middleware Trait

```rust
#[async_trait]
pub trait Middleware: Send + Sync {
    async fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult;
    async fn on_response(&self, ctx: &RequestContext, res: &mut Response) {}
}
```

The two methods are the two phases, both `async` so they can await real I/O
(a database session lookup, a remote key check) without blocking the runtime —
a body with no `.await` compiles the same:

- **`execute`** (required) runs before the handler, front to back in
  registration order. Return `MiddlewareResult::Error` to stop the pipeline
  with your own response.
- **`on_response`** (optional — the default does nothing) runs once the
  response exists, in **reverse** registration order, for every request whose
  `execute` ran: rejections, 404/405 and handler panics included, WebSocket
  upgrades excluded (there is no `Response`). It runs *before* the lifecycle
  log, the after-hooks and the telemetry counters, so a status you rewrite
  there is what gets logged.

Every `impl Middleware` needs `#[async_trait]` above it (and the matching
import). A struct that implements only `execute` needs no `on_response`.

For observation that has no business touching the `Response` (metrics, audit
writes) implement `AfterRequestHook` (below) instead; it also hands you the
final status and duration in one call.

### MiddlewareResult

```rust
pub enum MiddlewareResult {
    Next(Option<MiddlewareState>),  // Continue to next middleware/handler
    Error(Response),                 // Stop and return response immediately
}
```

```rust
struct TimingMiddleware;

#[async_trait]
impl Middleware for TimingMiddleware {
    async fn execute(&self, _ctx: &mut RequestContext) -> MiddlewareResult {
        // The frame the framework already set when the request arrived.
        MiddlewareResult::Next(None)
    }

    async fn on_response(&self, ctx: &RequestContext, res: &mut Response) {
        // The verdict needs the response, so it lands in the response phase.
        res.headers.push((
            "X-Response-Time".to_string(),
            format!("{}ms", ctx.start_time.elapsed().as_millis()),
        ));
    }
}

struct AddHeaderMiddleware;

#[async_trait]
impl Middleware for AddHeaderMiddleware {
    async fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Store data to be used later (values are multi-valued headers)
        ctx.headers
            .insert("X-Custom".to_string(), vec!["value".to_string()]);
        MiddlewareResult::Next(None)
    }
}

// Chain middleware
router = router
    .add_middleware(TimingMiddleware)
    .add_middleware(LoggerMiddleware)
    .add_middleware(AddHeaderMiddleware);
```

## AfterRequestHook

Both middleware phases are async. `AfterRequestHook` is the observation-only
counterpart: it runs after completion with the final status and duration, and
cannot modify the response — which is what makes it safe for fire-and-forget.

```rust
use async_trait::async_trait;
use std::time::Duration;

// Audit Logger Hook: Persists HTTP request metadata to a DB or external log service
pub struct AuditLogHook;

#[async_trait]
impl AfterRequestHook for AuditLogHook {
    async fn call(&self, ctx: &RequestContext, status: u16, duration: Duration) {
        // Asynchronously save to DB or audit system
        tokio::spawn({
            let path = ctx.path.clone();
            let method = ctx.method.clone();
            async move {
                println!(
                    "📜 [AUDIT LOG] {} {} -> Status: {} (Took {}ms)",
                    method, path, status, duration.as_millis()
                );
            }
        });
    }
}

// Performance Monitoring Hook: Triggers alerts for slow requests
pub struct SlowRequestNotifier {
    pub threshold: Duration,
}

#[async_trait]
impl AfterRequestHook for SlowRequestNotifier {
    async fn call(&self, ctx: &RequestContext, status: u16, duration: Duration) {
        if duration >= self.threshold {
            eprintln!(
                "⚠️ [WARN SLOW ROUTE] Path '{}' took {:?} to respond!",
                ctx.path, duration
            );
            // Can call async webhook, slack notification, or emit event
        }
    }
}
```

### Register Hooks

Rust

```rust
#[launch]
async fn main() {
    let router = Router::new()
        // Register middlewares (Before Hooks)
        .add_middleware(AuthMiddleware)
        // Register async after-hooks (After Hooks)
        .add_after_hook(AuditLogHook)
        .add_after_hook(SlowRequestNotifier {
            threshold: Duration::from_millis(500), // Warn if > 500ms
        });

}
```