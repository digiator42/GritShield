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

/// Both phases are `async` (`#[async_trait]`), so a middleware can await real
/// I/O — a database session lookup, a remote key check — without blocking the
/// runtime. A body with no `.await` compiles the same and costs only the
/// boxed future; every `impl` needs `#[async_trait]` above it.
#[async_trait]
pub trait Middleware: Send + Sync {
    /// The request phase. Runs front-to-back before the handler; return
    /// `MiddlewareResult::Error` to short-circuit with your own response.
    async fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult;

    /// The response phase — mutate the `Response` before it is written.
    ///
    /// Invariants the server maintains for you:
    ///
    /// * Runs for every request whose `execute` ran — including rejection
    ///   responses, 404/405, handler panics, and `MiddlewareResult::Error`
    ///   responses — and never for requests that bypassed middleware entirely
    ///   (malformed requests, WebSocket upgrades, which have no `Response`).
    /// * Registration order **reversed**, so the first-registered middleware
    ///   finalizes last, and only over the middlewares that actually executed.
    /// * Runs *before* `log_lifecycle`, `AfterRequestHook`s and telemetry
    ///   observe the response, so a status you rewrite here is what gets
    ///   logged and counted.
    ///
    /// For observation without access to the `Response` (metrics, audit
    /// writes) use [`AfterRequestHook`] instead.
    async fn on_response(&self, _ctx: &RequestContext, _res: &mut Response) {}
}

#[async_trait]
impl Middleware for Box<dyn Middleware> {
    async fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Delegate to the inner middleware
        self.as_ref().execute(ctx).await
    }

    // Must forward explicitly: a defaulted method is inherited here, so
    // skipping it would silently no-op `on_response` for every middleware
    // registered through `add_middleware` (which boxes them).
    async fn on_response(&self, ctx: &RequestContext, res: &mut Response) {
        self.as_ref().on_response(ctx, res).await
    }
}

#[async_trait]
impl Middleware for Arc<dyn Middleware> {
    async fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        self.as_ref().execute(ctx).await
    }

    async fn on_response(&self, ctx: &RequestContext, res: &mut Response) {
        self.as_ref().on_response(ctx, res).await
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
