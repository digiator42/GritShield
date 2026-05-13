use crate::protocol::{request::Request, response::Response};
use crate::security::jwt::JwtHandler;
use crate::security::xss::Sanitizer;
use colored::*;

pub enum MiddlewareResult {
    Next,            // Continue to next middleware/handler
    Error(Response), // Stop and return error immediately
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
            return MiddlewareResult::Next;
        }

        // Extract Header
        if let Some(auth_header) = req.headers.get("authorization") {
            if auth_header.starts_with("Bearer ") {
                let token = &auth_header[7..];

                // Verify Token
                match self.jwt_handler.verify(token) {
                    Ok(claims) => {
                        println!("[AUTH] Verified user: {}", claims.sub);
                        return MiddlewareResult::Next;
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
        MiddlewareResult::Next
    }
}
