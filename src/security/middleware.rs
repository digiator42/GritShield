use std::sync::{Arc, Mutex};

use crate::protocol::{request::Request, response::Response};
use crate::routing::trie::RequestContext;
use crate::security::jwt::{Claims, JwtHandler};
use crate::security::rate_limit::RateLimiter;
use crate::security::session::{Session, SessionStore};
use crate::security::xss::Sanitizer;
use colored::*;

pub enum MiddlewareResult {
    Next(Option<MiddlewareState>), // State can hold session data, claims, or both
    Error(Response),               // Stop and return error immediately
}

// A state packer to carry data down the pipe safely
pub struct MiddlewareState {
    pub session: Option<Arc<Mutex<Session>>>,
    pub claims: Option<Claims>,
}

pub trait Middleware: Send + Sync {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult;
}

pub trait AfterRequestHook: Send + Sync {
    fn call(&self, ctx: &RequestContext, status: u16, duration: std::time::Duration);
}

pub struct AuthMiddleware {
    pub jwt_handler: JwtHandler,
    pub public_paths: Vec<String>, // List of open routes
}

impl AuthMiddleware {
    /// Internal helper to evaluate whether a path matches a whitelisted path rule
    fn is_public(&self, incoming_path: &str) -> bool {
        let incoming_segments: Vec<&str> =
            incoming_path.split('/').filter(|s| !s.is_empty()).collect();

        for rule in &self.public_paths {
            let rule_segments: Vec<&str> = rule.split('/').filter(|s| !s.is_empty()).collect();

            // Handle explicit root match ("/")
            if rule == "/" && incoming_path == "/" {
                return true;
            }

            let mut matches = true;
            for (i, rule_seg) in rule_segments.iter().enumerate() {
                // If we hit your wildcard token variant, everything past this point matches!
                if *rule_seg == "*" || rule_seg.starts_with(':') && rule_seg.contains('*') {
                    return true;
                }

                // If the incoming path is shorter than the rule segment check, it's not a match
                if i >= incoming_segments.len() {
                    matches = false;
                    break;
                }

                // Standard exact segment match evaluation
                if rule_seg != &incoming_segments[i] {
                    matches = false;
                    break;
                }
            }

            // If we checked all rule segments perfectly and length matches, it's a valid match
            if matches && incoming_segments.len() == rule_segments.len() {
                return true;
            }
        }

        false
    }
}

impl Middleware for AuthMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Smart match calculation using exact segment-by-segment mapping
        if self.is_public(&ctx.req.path) {
            //
            return MiddlewareResult::Next(None);
        }

        // Extract Header
        if let Some(auth_header) = ctx.headers.get("authorization") {
            //
            if auth_header.starts_with("Bearer ") {
                let token = &auth_header[7..];

                // Verify Token
                match self.jwt_handler.verify(token) {
                    //
                    Ok(claims) => {
                        //
                        println!("[AUTH] Verified user: {}", claims.sub);

                        ctx.claims = Some(claims);

                        let forward_state = MiddlewareState {
                            session: None,
                            claims: ctx.claims.clone(),
                        };

                        return MiddlewareResult::Next(Some(forward_state));
                    }
                    Err(e) => {
                        println!("[AUTH] Rejected: {}", e);
                    }
                }
            }
        }

        // Fail: Short-circuit the request safely
        let err_body = Sanitizer::trust("<h1>401 Unauthorized</h1>"); //
        MiddlewareResult::Error(Response::new(401, err_body)) //
    }
}

pub struct LoggerMiddleware;

impl Middleware for LoggerMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        println!(
            "[LOG] {} request to {}",
            format!("{:?}", ctx.req.method).blue(),
            ctx.req.path.yellow()
        );
        MiddlewareResult::Next(None)
    }
}
pub struct SessionMiddleware {
    pub store: Arc<SessionStore>,
}

impl Middleware for SessionMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        let session_id = ctx
            .headers
            .get("cookie")
            .and_then(|c| c.split("; ").find(|s| s.starts_with("session_id=")))
            .map(|s| s["session_id=".len()..].to_string());

        // We only look up. We don't 'create' yet.
        let store = self.store.sessions.lock().unwrap();
        if let Some(sid) = session_id {
            if let Some(session_ptr) = store.get(&sid) {
                ctx.session = Some(Arc::clone(session_ptr));
                return MiddlewareResult::Next(Some(MiddlewareState {
                    session: Some(Arc::clone(session_ptr)),
                    claims: None, // No JWT claims here
                }));
            }
        }

        // No session found? Just continue without one.
        MiddlewareResult::Next(None)
    }
}

pub struct RateLimitMiddleware {
    pub limiter: RateLimiter,
}

impl Middleware for RateLimitMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // use req.headers.get("x-forwarded-for")
        // or the peer_addr from the TcpStream.
        let ip = ctx
            .req
            .headers
            .get("host")
            .unwrap_or(&"unknown".to_string())
            .clone();

        if self.limiter.is_allowed(ip) {
            // we pass None here as Rate Limiting doesn't create a session.
            MiddlewareResult::Next(None)
        } else {
            let err_body =
                Sanitizer::trust("<h1>429 Too Many Requests</h1><p>Slow down, friend.</p>");
            let mut res = Response::new(429, err_body);
            res.headers
                .push(("Retry-After".to_string(), "60".to_string()));

            MiddlewareResult::Error(res)
        }
    }
}

pub struct MetricsTracker;

impl AfterRequestHook for MetricsTracker {
    fn call(&self, ctx: &RequestContext, status: u16, _: std::time::Duration) {
        if status >= 500 {
            eprintln!(
                "🚨 [ALERT] Critical server failure detected on path: {}",
                ctx.req.path
            );
        } else if status >= 400 {
            eprintln!(
                "🚨 [ALERT] server failure detected on path: {}",
                ctx.req.path
            );
        }
    }
}