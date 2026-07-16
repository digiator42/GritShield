// src/middleware/utils.rs
use std::sync::{Arc, Mutex};
use crate::http::response::Response;
use crate::routing::trie::RequestContext;
use crate::security::jwt::Claims;
use crate::security::session::Session;

pub enum MiddlewareResult {
    Next(Option<MiddlewareState>), // State can hold session data, claims, or both
    Error(Response),               // Stop and return error immediately
}

// A state packer to carry data down the pipe safely
pub struct MiddlewareState {
    pub session: Option<Arc<Mutex<Session>>>,
    pub claims: Option<Claims>,
    pub session_was_stale: bool,
}

pub trait Middleware: Send + Sync {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult;
}

pub trait AfterRequestHook: Send + Sync {
    fn call(&self, ctx: &RequestContext, status: u16, duration: std::time::Duration);
}