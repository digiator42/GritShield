use crate::http::request::HttpMethod;
use crate::http::response::Response;
use crate::routing::trie::RequestContext;
use crate::middleware::{Middleware, MiddlewareResult};

pub struct CorsMiddleware {
    allowed_origins: Vec<String>,
}

impl CorsMiddleware {
    /// Accept an array or vector of strings during initialization
    pub fn new(origins: Vec<String>) -> Self {
        Self {
            allowed_origins: origins,
        }
    }
}

impl Middleware for CorsMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Extract the origin the browser is currently calling from
        let inbound_origin = ctx.req.headers.get("Origin").cloned().unwrap_or_default();

        // Determine the target match. If the domain is whitelisted, echo it!
        // Otherwise, fallback to your primary origin safely.
        let dynamic_origin = if self.allowed_origins.contains(&inbound_origin) {
            inbound_origin
        } else {
            self.allowed_origins
                .first()
                .cloned()
                .unwrap_or_else(|| "http://localhost:3000".to_string())
        };

        // Handle preflight checks
        if ctx.req.method == HttpMethod::OPTIONS || ctx.req.method == HttpMethod::UNKNOWN {
            let mut res = Response::ok("Preflight Allowed");

            res.headers.push((
                "Access-Control-Allow-Origin".to_string(),
                dynamic_origin.clone(),
            ));
            res.headers.push((
                "Access-Control-Allow-Methods".to_string(),
                "POST, GET, OPTIONS, PUT, PATCH, DELETE".to_string(),
            ));
            res.headers.push((
                "Access-Control-Allow-Headers".to_string(),
                "Content-Type, Authorization".to_string(),
            ));
            res.headers
                .push(("Access-Control-Max-Age".to_string(), "86400".to_string()));

            return MiddlewareResult::Error(res);
        }

        // Handle standard operations
        ctx.headers
            .insert("Access-Control-Allow-Origin".to_string(), dynamic_origin);
        ctx.headers.insert(
            "Access-Control-Allow-Methods".to_string(),
            "GET, POST, PUT, PATCH, DELETE, OPTIONS".to_string(),
        );
        ctx.headers.insert(
            "Access-Control-Allow-Headers".to_string(),
            "Content-Type, Authorization".to_string(),
        );

        MiddlewareResult::Next(None)
    }
}