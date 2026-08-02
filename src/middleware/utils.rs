use crate::http::response::Response;
use crate::routing::engine::RequestContext;
use crate::security::jwt::Claims;
use crate::security::session::Session;
use sea_orm_migration::async_trait::async_trait;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

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

impl Middleware for Box<dyn Middleware> {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Delegate to the inner middleware
        self.as_ref().execute(ctx)
    }
}

impl Middleware for Arc<dyn Middleware> {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        self.as_ref().execute(ctx)
    }
}

#[async_trait]
pub trait AfterRequestHook: Send + Sync {
    async fn call(&self, ctx: &RequestContext, status: u16, duration: Duration);
}

#[async_trait]
impl AfterRequestHook for Box<dyn AfterRequestHook> {
    async fn call(&self, ctx: &RequestContext, status: u16, duration: Duration) {
        self.as_ref().call(ctx, status, duration).await
    }
}

#[async_trait]
impl AfterRequestHook for Arc<dyn AfterRequestHook> {
    async fn call(&self, ctx: &RequestContext, status: u16, duration: Duration) {
        self.as_ref().call(ctx, status, duration).await
    }
}
