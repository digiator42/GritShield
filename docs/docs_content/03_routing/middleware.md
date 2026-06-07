
Middleware runs before handlers and can modify the request context, add data, or reject requests.

## Middleware Trait

```rust
pub trait Middleware: Send + Sync {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult;
}
```

## MiddlewareResult

```rust
pub enum MiddlewareResult {
    Next(Option<MiddlewareState>),  // Continue to next middleware/handler
    Error(Response),                 // Stop and return response immediately
}
```

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

Run code after request completion:

```rust
pub trait AfterRequestHook: Send + Sync {
    fn call(&self, ctx: &RequestContext, status: u16, duration: Duration);
}

struct MetricsHook;

impl AfterRequestHook for MetricsHook {
    fn call(&self, ctx: &RequestContext, status: u16, duration: Duration) {
        metrics.record_request(ctx.req.path, status, duration);
    }
}

router = router.add_after_hook(Box::new(MetricsHook));
```