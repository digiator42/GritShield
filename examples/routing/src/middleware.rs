//! Writing middleware and after-request hooks.
//!
//! The `Middleware` trait is **synchronous** and returns a verdict:
//!
//! ```text
//! MiddlewareResult::Next(state) ──▶ carry on down the pipeline
//! MiddlewareResult::Error(resp) ──▶ stop here, `resp` is the client's answer
//! ```
//!
//! That is the entire contract. A middleware may reject a request, annotate the
//! context for handlers behind it, or do nothing at all.
//!
//! The one thing it cannot do is invent a channel. `MiddlewareState` carries
//! `session`, `claims` and `session_was_stale` — nothing else — so if you need
//! to hand data to a handler, either annotate the `RequestContext` (as
//! `RequestIdMiddleware` below does with `ctx.headers`) or hold an
//! `Arc<Mutex<_>>` inside your middleware struct.
//!
//! ## Annotating vs. responding
//!
//! There are two ways to set a header, and they are not interchangeable.
//!
//! **On the `Response`** — for values the *handler* decides on. Use
//! `Response::with_header`. This is the ordinary way.
//!
//! **On `ctx.headers`** — for values *middleware* decides on. The server builds
//! the response after your handler returns, so a middleware has no handle on it;
//! inserting into `ctx.headers` is the supported channel, and the server
//! promotes those names to real response headers afterwards
//! (`Response::merge_middleware_headers`).
//!
//! The promotion is deliberately narrow, and the narrowness is the point:
//!
//! ```text
//! client sent: Cookie, Authorization, Host, User-Agent, ...
//!   ──▶ never promoted. Reflecting a session id or bearer token into the
//!        response would hand it to any proxy or CDN that caches the reply.
//! middleware added: x-request-id
//!   ──▶ promoted, unless the handler already set a matching name.
//! ```
//!
//! Name matching is case-insensitive, so a handler's `with_header("X-Request-Id")`
//! suppresses the middleware's `x-request-id` instead of emitting the value
//! twice. Because the check is per name and runs once, a multi-valued middleware
//! header still forwards all of its values.
//!
//! So: put it on the `Response` if a handler owns the value, and on
//! `ctx.headers` if middleware owns it. What you must not do is treat
//! `ctx.headers` as a general-purpose response-header bag — it starts out as a
//! copy of the request, and anything that lands there is a candidate for the
//! wire.

use async_trait::async_trait;
use gritshield::http::response::HttpStatus;
use gritshield::middleware::{AfterRequestHook, Middleware, MiddlewareResult};
use gritshield::prelude::*;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Tags every request with a correlation id, visible to handlers.
///
/// ```bash
/// curl -s http://127.0.0.1:8081/mw/echo-request-id
/// # {"request_id":"req-000001","read_via":"ctx.header(\"x-request-id\")"}
/// ```
///
/// The counter is plain `Relaxed` ordering: it only has to hand out distinct
/// numbers, not establish a total order across threads.
pub struct RequestIdMiddleware {
    counter: AtomicU64,
}

impl RequestIdMiddleware {
    pub fn new() -> Self {
        Self {
            counter: AtomicU64::new(0),
        }
    }
}

impl Default for RequestIdMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for RequestIdMiddleware {
    async fn on_request(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        let n = self.counter.fetch_add(1, Ordering::Relaxed) + 1;

        // Downstream handlers see this through `ctx.header("x-request-id")`.
        // It is a view onto the request, not onto the response.
        ctx.headers.insert(
            "x-request-id".to_string(),
            vec![format!("req-{n:06}")],
        );

        // `Next(None)` means "carry on, and I have no session or claims state to
        // add". Passing `None` matters: it is what keeps this middleware from
        // clearing a session an earlier one established.
        MiddlewareResult::Next(None)
    }
}

/// Demonstrates reading what middleware put on the context, and setting a
/// response header for real.
///
/// Both headers appear exactly once, for the two different reasons above.
pub struct HeaderController;

#[controller("/mw")]
impl HeaderController {
    /// Reads the id middleware attached, then echoes it back on the response.
    ///
    /// ```bash
    /// curl -i http://127.0.0.1:8081/mw/echo-request-id
    /// # body:   {"request_id":"req-000019", ...}
    /// # header: X-Request-Id: req-000019        <- once, not twice
    /// ```
    ///
    /// The handler sets it, so the middleware's lowercased copy of the same
    /// name is suppressed on the way out. Drop the `with_header` line and you
    /// get the same single header from the middleware's annotation alone —
    /// which is the point: the two routes do not conflict.
    #[get("/echo-request-id")]
    pub async fn echo_request_id(ctx: RequestContext) -> Response {
        let request_id = ctx.header("x-request-id").unwrap_or("no-middleware-ran");

        Response::json(
            HttpStatus::Ok,
            &json!({
                "request_id": request_id,
                "read_via": "ctx.header(\"x-request-id\")",
            }),
        )
        .with_header("X-Request-Id", request_id)
    }
}

/// A middleware that can also say no.
///
/// This is the half of the trait people forget: `on_request` is not obliged to
/// continue. Rejecting here means the handler never runs, and — because
/// middleware short-circuits — nothing registered after it runs either.
///
/// It is configured with the subtree it *guards* rather than the one it exempts,
/// so the rest of this guide stays reachable without ceremony:
///
/// ```bash
/// curl -i http://127.0.0.1:8081/secure/report
/// # 401 {"error":"missing or invalid X-Api-Key"}
///
/// curl -i -H "X-Api-Key: dev-key" http://127.0.0.1:8081/secure/report
/// # 200 {"secret":"..."}
/// ```
///
/// Note the asymmetry with authentication middleware: this key travels in a
/// header the caller sets deliberately, so it needs no CSRF token. That is
/// exactly why it must not be the *only* thing standing in front of a
/// cookie-authenticated session.
pub struct ApiKeyMiddleware {
    expected: String,
    protected_prefix: String,
}

impl ApiKeyMiddleware {
    pub fn new(expected: &str, protected_prefix: &str) -> Self {
        Self {
            expected: expected.to_string(),
            protected_prefix: protected_prefix.to_string(),
        }
    }
}

#[async_trait]
impl Middleware for ApiKeyMiddleware {
    async fn on_request(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Outside the guarded subtree there is nothing to check.
        if !ctx.req.path.starts_with(&self.protected_prefix) {
            return MiddlewareResult::Next(None);
        }

        match ctx.header("x-api-key") {
            Some(key) if key == self.expected => MiddlewareResult::Next(None),
            _ => MiddlewareResult::Error(Response::unauthorized(
                &std::collections::HashMap::from([(
                    "error",
                    "missing or invalid X-Api-Key",
                )]),
            )),
        }
    }
}

/// Routes behind the API-key middleware, to show what the guard buys.
pub struct SecureController;

#[controller("/secure")]
impl SecureController {
    /// Reached only with a valid `X-Api-Key`.
    ///
    /// ```bash
    /// curl -i -H "X-Api-Key: dev-key" http://127.0.0.1:8081/secure/report
    /// ```
    #[get("/report")]
    pub async fn report(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({
                "secret": "only rendered because the middleware let this through",
                // The same id middleware annotated the context, unchanged.
                "request_id": ctx.header("x-request-id"),
            }),
        )
    }
}

/// ## After-request hooks
///
/// Where `Middleware` decides whether a request proceeds, an `AfterRequestHook`
/// observes what happened. It runs once the response status and duration are
/// known, which is exactly the information a before-hook does not have.
///
/// The trait is `async` and annotated with `#[async_trait]`, so an
/// implementation can await — which is what makes it the right place to write
/// an audit line to a database or forward it to a collector.
///
/// ```bash
/// curl -s http://127.0.0.1:8081/api/ping > /dev/null
/// # server stdout: AUDIT GET /api/ping -> 200 in 0ms
/// ```
pub struct AuditLogHook;

#[async_trait]
impl AfterRequestHook for AuditLogHook {
    async fn call(&self, ctx: &RequestContext, status: u16, duration: Duration) {
        // Runs after the response is built, so failures here cannot affect the
        // client — keep it that way, and do not block the reactor on I/O.
        println!(
            "AUDIT {:?} {} -> {} in {}ms",
            ctx.req.method,
            ctx.req.path,
            status,
            duration.as_millis()
        );
    }
}

/// Watches for slow handlers. Same trait, different use: it has a threshold and
/// only speaks up when it is crossed.
pub struct SlowRequestNotifier {
    pub threshold: Duration,
}

#[async_trait]
impl AfterRequestHook for SlowRequestNotifier {
    async fn call(&self, ctx: &RequestContext, status: u16, duration: Duration) {
        if duration >= self.threshold {
            eprintln!(
                "SLOW {:?} {} took {}ms and returned {}",
                ctx.req.method,
                ctx.req.path,
                duration.as_millis(),
                status
            );
        }
    }
}