use crate::protocol::{request::Request, response::Response};

pub enum MiddlewareResult {
    Next,            // Continue to next middleware/handler
    Error(Response), // Stop and return error immediately
}

pub trait Middleware: Send + Sync {
    fn execute(&self, req: &Request) -> MiddlewareResult;
}

