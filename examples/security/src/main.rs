//! # GritShield security developer guide
//!
//! A single runnable service that demonstrates the four security features
//! together, in the order a real application should apply them:
//!
//! | Module | Feature |
//! |--------|---------|
//! | [`xss`] | input sanitisation and output encoding |
//! | [`csrf`] | session authentication and anti-forgery tokens |
//! | [`abuse`] | rate limiting and IP blacklisting |
//!
//! ## Middleware order is the point
//!
//! Middleware executes **in the order it is added**, and every step below can
//! reject a request on its own. So the pipeline is ordered cheapest-first:
//!
//! ```text
//!   blacklisted IP?      ──▶ 403, no session touched, no crypto done
//!   over rate limit?     ──▶ 429, cheap counters, no handler state
//!   cross-origin?        ──▶ CORS headers applied
//!   public route?        ──▶ mint a session, then straight to the handler
//!   unauthenticated?     ──▶ 303 to the login page
//!   missing/bad CSRF?    ──▶ 403, state-changing request only
//!   ──▶ handler
//! ```
//!
//! Reverse the first two and a flood of banned IPs costs you a signed-cookie
//! HMAC and a session-store lookup each. On a busy service that is the
//! difference between shedding load and becoming the bottleneck.
//!
//! ## One deliberate opt-in
//!
//! `AuthMiddleware::new_session` leaves `enable_csrf` **false**. That is a
//! compatibility default, not a recommendation — in session mode a browser
//! sends your cookie without asking, which is precisely the situation CSRF
//! exists for. So this example turns it on explicitly, and the docs at
//! `docs/docs_content/03_security` currently describe the guard as automatic
//! in session mode, which is worth knowing about if you follow them.
//!
//! Run with:
//!
//! ```bash
//! cargo run
//! ```

use gritshield::middleware::{
    AuthMiddleware, CorsMiddleware, IPBlacklistMiddleware, RateLimitMiddleware,
};
use gritshield::prelude::*;
use gritshield::security::rate_limit::RateLimiter;
use std::time::Duration;

mod abuse;
mod csrf;
mod xss;

/// Routes an anonymous visitor may reach. Everything else requires a session.
///
/// The sharp edge of this middleware is worth stating plainly, because it is
/// easy to get backwards: a public route bypasses **both** gates. Listing a path
/// here waives the authentication redirect *and* the CSRF guard — `STEP 1` in
/// `src/middleware/auth.rs` returns before the token check is ever reached.
///
/// Verified behaviour of this example:
///
/// - anonymous `POST /auth/session`, no token → `200`, logged in
/// - anonymous `POST /auth/preferences`, no token → `303` to the login page
/// - authenticated `POST /auth/preferences`, no token → `403`
///
/// So public paths are for routes that are genuinely safe to call without a
/// token. The moment a handler changes state belonging to a signed-in user, it
/// must not be public — that is the case CSRF exists to protect.
const PUBLIC_PATHS: &[&str] = &[
    "/health",
    "/xss/unsafe",
    "/xss/safe",
    "/xss/over-escaped",
    "/xss/trusted",
    "/xss/url",
    "/xss/profile",
    "/auth/login",
    "/auth/session",
    "/auth/csrf-token",
    "/abuse/ping",
    "/abuse/expensive",
    "/abuse/whoami",
];

#[tokio::main]
async fn main() {
    // 1. Reject known-bad callers first. Nothing else runs for them.
    //
    //    Reads are IPs, not patterns. A real deployment would load this from
    //    config or the database rather than hard-coding it; the point is that
    //    it is a middleware, so it also covers admin routes and MCP endpoints.
    let blacklist = IPBlacklistMiddleware::new(vec![
        "203.0.113.66", // TEST-NET-3, reserved for documentation
        "198.51.100.23",
    ]);

    // 2. Cap request volume per client. 100 requests per minute is generous
    //    for the demo; it exists to be tripped.
    //
    //    The bucket key is `resolve_client_ip()`, so it is per-caller rather
    //    than global — one noisy client cannot exhaust everyone else's budget.
    let rate_limit = RateLimitMiddleware {
        limiter: RateLimiter::new(100, Duration::from_secs(60)),
    };

    // 3. Browser-facing CORS. Only needed because the guide is meant to be
    //    called from a page on another origin; a same-origin service can drop
    //    this line entirely.
    let cors = CorsMiddleware::new(vec!["http://localhost:5173".to_string()]);

    // 4. Sessions, authentication and the CSRF guard.
    let mut auth = AuthMiddleware::new_session(
        PUBLIC_PATHS.iter().map(|p| p.to_string()).collect(),
        Some("/auth/login"),
    );

    // THE opt-in that makes session mode safe. Off by default; see the module
    // docs for why that default is questionable.
    auth.enable_csrf = true;

    // Chained in the order above, because `add_middleware` pushes onto a Vec and
    // the pipeline walks it front to back.
    //
    // `Router::new()` also discovers every `#[controller]` block in this binary
    // through `inventory`, so no route is registered by hand here.
    let router = Router::new()
        .add_middleware(blacklist)
        .add_middleware(rate_limit)
        .add_middleware(cors)
        .add_middleware(auth);

    ignite("127.0.0.1", "8080", router).await;
}

pub struct HealthController;

/// Registered by `Router::new()` through `inventory`, not by hand — the same
/// discovery mechanism that finds `#[controller]` blocks anywhere in the
/// binary. See `examples/dependency_injection` for how that differs from the
/// compile-time `WireContainer` graph.
#[controller("/")]
impl HealthController {
    /// Handy for scripted checks and for confirming the server is up before
    /// walking through the other endpoints.
    #[get("/health")]
    pub async fn health() -> Response {
        Response::json(
            gritshield::http::response::HttpStatus::Ok,
            &serde_json::json!({
                "status": "ok",
                "guides": {
                    "xss": ["/xss/unsafe", "/xss/safe", "/xss/profile"],
                    "csrf": ["/auth/login", "/auth/csrf-token", "/auth/session"],
                    "abuse": ["/abuse/ping", "/abuse/expensive"],
                }
            }),
        )
    }
}