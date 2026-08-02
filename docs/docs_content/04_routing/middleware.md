
Middleware runs before handlers and can modify the request context, add data, or reject requests.


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
pub trait Middleware: Send + Sync {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult;
}
```

### MiddlewareResult

```rust
pub enum MiddlewareResult {
    Next(Option<MiddlewareState>),  // Continue to next middleware/handler
    Error(Response),                 // Stop and return response immediately
}
```

```rust
struct TimingMiddleware;

impl Middleware for TimingMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        ctx.start_time = std::time::Instant::now();
        MiddlewareResult::Next(None)
    }
}

struct AddHeaderMiddleware;

impl Middleware for AddHeaderMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Store data to be used later
        ctx.headers.insert("X-Custom".to_string(), "value".to_string());
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

Middleware runs before requests, `AfterRequestHook` run after request completion.

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