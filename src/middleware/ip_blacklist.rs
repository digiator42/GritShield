use std::collections::HashSet;
use std::sync::atomic::Ordering;

use crate::http::response::Response;
use crate::routing::trie::RequestContext;
use crate::security::xss::Sanitizer;
use crate::middleware::{Middleware, MiddlewareResult};
use crate::error;

pub struct IPBlacklistMiddleware {
    // Using HashSet for high-performance O(1) lookups
    pub blacklisted_ips: HashSet<String>,
}

impl IPBlacklistMiddleware {
    /// Constructor helper to instantiate the blacklist layer smoothly
    pub fn new(ips: Vec<&str>) -> Self {
        let mut set = HashSet::new();
        for ip in ips {
            set.insert(ip.to_string());
        }
        Self {
            blacklisted_ips: set,
        }
    }
}

impl Middleware for IPBlacklistMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Leverage your secure IP resolver from earlier
        let client_ip = ctx.resolve_client_ip();

        // Check the HashSet instantly
        if self.blacklisted_ips.contains(&client_ip) {
            error!(
                "[SECURITY ALERT] Blocked request attempt from blacklisted IP: {}",
                client_ip
            );

            ctx.telemetry
                .total_blocked_ips
                .fetch_add(1, Ordering::SeqCst);

            let err_body = Sanitizer::trust(
                "<h1>403 Forbidden</h1>\
                 <p>Access denied. Your IP address has been blocked.</p>",
            );

            let mut res = Response::new(403, err_body);
            res.headers.push((
                "Content-Type".to_string(),
                "text/html; charset=utf-8".to_string(),
            ));

            // Drop connection right here! Do not proceed to handlers or next middlewares
            MiddlewareResult::Error(res)
        } else {
            // All clear. Move down the execution pipeline chain smoothly
            MiddlewareResult::Next(None)
        }
    }
}