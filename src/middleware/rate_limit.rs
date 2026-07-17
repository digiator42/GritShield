use crate::http::response::Response;
use crate::routing::engine::RequestContext;
use crate::security::rate_limit::RateLimiter;
use crate::security::xss::Sanitizer;
use crate::middleware::{Middleware, MiddlewareResult};

pub struct RateLimitMiddleware {
    pub limiter: RateLimiter,
}

impl Middleware for RateLimitMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // SECURELY resolve the true user identity string
        let client_ip = ctx.resolve_client_ip();

        // logging to see who is making requests
        println!(
            "[GRITSHIELD RATE-LIMIT] Evaluating limits for bucket identifier: {}",
            client_ip
        );

        if self.limiter.is_allowed(client_ip) {
            // Pass execution forward down the routing chain
            MiddlewareResult::Next(None)
        } else {
            // Client hit the ceiling limit! Push back with an explicit HTTP 429 back-off directive
            let err_body = Sanitizer::trust(
                "<h1>429 Too Many Requests</h1>\
                 <p>Slow down, friend. Your API bucket limits have been exhausted.</p>",
            );

            let mut res = Response::new(429, err_body);

            // Instruct downstream agents/browsers exactly how long to wait before trying again
            res.headers
                .push(("Retry-After".to_string(), "60".to_string()));
            res.headers.push((
                "Content-Type".to_string(),
                "text/html; charset=utf-8".to_string(),
            ));

            MiddlewareResult::Error(res)
        }
    }
}