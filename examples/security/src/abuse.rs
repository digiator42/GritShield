//! Rate limiting and IP blacklisting.
//!
//! These two are the cheapest defences in the whole framework: both are pure
//! middleware, neither needs a database, and both run before your handler — so
//! a flood costs the attacker a socket, not your database connection.
//!
//! Ordering is the lesson here. Middleware runs in the order it was added, so
//! the cheap rejections come first and authentication never runs for a request
//! that was going to be turned away anyway. See `main.rs`.
//!
//! Open `src/abuse.rs` next to this file: it is the guide.

use gritshield::http::response::HttpStatus;
use gritshield::prelude::*;
use serde_json::json;

pub struct AbuseController;

#[controller("/abuse")]
impl AbuseController {
    /// Every request through here consumes one token from the limiter
    /// configured in `main.rs`. Call it in a loop and the surplus answers `429`.
    ///
    /// ```bash
    /// # watch the limit trip
    /// for i in $(seq 1 200); do
    ///   curl -s -o /dev/null -w "%{http_code}\n" http://localhost:8080/abuse/ping
    /// done | sort | uniq -c
    /// ```
    #[get("/ping")]
    pub async fn ping(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({
                "pong": true,
                "client_ip": ctx.resolve_client_ip(),
                "note": "rate limited by RateLimitMiddleware",
            }),
        )
    }

    /// A deliberately expensive endpoint, to show the limiter doing its job.
    ///
    /// The point of a rate limit is that it is charged per caller, not per
    /// route: the limiter is installed once on the router, so this route and
    /// `/abuse/ping` draw from the same budget.
    #[get("/expensive")]
    pub async fn expensive(ctx: RequestContext) -> Response {
        // Stand-in for real work. In an async runtime this belongs behind a
        // spawn_blocking rather than on the reactor thread.
        let mut acc = 0u64;
        for n in 0..2_000_000u64 {
            acc = acc.wrapping_add(n);
        }

        Response::json(
            HttpStatus::Ok,
            &json!({ "computed": acc, "client_ip": ctx.resolve_client_ip() }),
        )
    }

    /// `resolve_client_ip` is what the limiter and the blacklist both key on.
    /// Worth understanding before you trust either.
    ///
    /// It prefers the leftmost entry of `X-Forwarded-For` and falls back to the
    /// socket address, with **no trusted-proxy allowlist**. Behind a real edge
    /// that is what you want; exposed directly to the internet it is not, since
    /// a client can send its own header and thereby choose both its blacklist
    /// verdict and its rate-limit bucket. Strip or overwrite the header at your
    /// edge before it reaches the app.
    ///
    /// The example below is exploitable on purpose, so you can see it:
    ///
    /// ```bash
    /// curl http://localhost:8080/abuse/whoami
    /// curl -H "X-Forwarded-For: 203.0.113.66" http://localhost:8080/abuse/whoami
    /// ```
    #[get("/whoami")]
    pub async fn whoami(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({
                "resolved_ip": ctx.resolve_client_ip(),
                "forwarded_for": ctx.header("x-forwarded-for"),
            }),
        )
    }
}