use std::sync::{Arc, Mutex};

use crate::protocol::{request::Request, response::Response};
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
    fn execute(&self, req: &Request) -> MiddlewareResult;
}

pub struct AuthMiddleware {
    pub jwt_handler: JwtHandler,
    pub public_paths: Vec<String>, // List of open routes
}

impl Middleware for AuthMiddleware {
    fn execute(&self, req: &Request) -> MiddlewareResult {
        // Check if the current path is in the whitelist
        // We use .starts_with to handle sub-paths or exact matches
        if self
            .public_paths
            .iter()
            .any(|path| req.path.starts_with(path))
        {
            return MiddlewareResult::Next(None);
        }

        // Extract Header
        if let Some(auth_header) = req.headers.get("authorization") {
            if auth_header.starts_with("Bearer ") {
                let token = &auth_header[7..];

                // Verify Token
                match self.jwt_handler.verify(token) {
                    Ok(claims) => {
                        println!("[AUTH] Verified user: {}", claims.sub);

                        // Pass the verified claims into the pipeline state instead of dropping them
                        let forward_state = MiddlewareState {
                            session: None, // No session, stateless
                            claims: Some(claims),
                        };

                        return MiddlewareResult::Next(Some(forward_state));
                    }
                    Err(e) => {
                        println!("[AUTH] Rejected: {}", e);
                    }
                }
            }
        }

        // Fail: Short-circuit the request
        let err_body = Sanitizer::trust("<h1>401 Unauthorized</h1>");
        MiddlewareResult::Error(Response::new(401, err_body))
    }
}

pub struct LoggerMiddleware;

impl Middleware for LoggerMiddleware {
    fn execute(&self, req: &Request) -> MiddlewareResult {
        println!(
            "[LOG] {} request to {}",
            format!("{:?}", req.method).blue(),
            req.path.yellow()
        );
        MiddlewareResult::Next(None)
    }
}
pub struct SessionMiddleware {
    pub store: Arc<SessionStore>,
}

impl Middleware for SessionMiddleware {
    fn execute(&self, req: &Request) -> MiddlewareResult {
        let session_id = req
            .headers
            .get("cookie")
            .and_then(|c| c.split("; ").find(|s| s.starts_with("session_id=")))
            .map(|s| s["session_id=".len()..].to_string());

        // We only look up. We don't 'create' yet.
        let store = self.store.sessions.lock().unwrap();
        if let Some(sid) = session_id {
            if let Some(session_ptr) = store.get(&sid) {
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
    fn execute(&self, req: &Request) -> MiddlewareResult {
        // use req.headers.get("x-forwarded-for")
        // or the peer_addr from the TcpStream.
        let ip = req
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
