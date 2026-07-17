use crate::core::logger::{self, LogLevel, log_request_summary};
use crate::middleware::{AfterRequestHook, Middleware};
use crate::routing::RequestContext;
use crate::routing::engine::fallback::PageHandlerFn;
use crate::routing::engine::Router;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

impl Router {
    pub fn mount_db(mut self, db: Arc<DatabaseConnection>) -> Self {
        self.db = Some(db);
        self
    }

    /// Premium builder to switch on detailed diagnostic server logs
    pub fn mount_logger(self, level: LogLevel) -> Self {
        logger::init(level);
        self
    }

    /// Register a global pipeline middleware by moving ownership
    pub fn add_middleware(mut self, middleware: impl Middleware + 'static) -> Self {
        self.middlewares.push(Box::new(middleware));
        self // Return ownership back out to the chain
    }

    /// Builder to mount custom post-execution lifecycle hooks
    pub fn add_after_hook(mut self, hook: Box<dyn AfterRequestHook>) -> Self {
        self.after_hooks.push(hook);
        self
    }

    /// Allows developers to attach a custom layout handler for unmatched 404 routes
    pub fn set_fallback(mut self, handler: PageHandlerFn) -> Self {
        self.fallback_handler = Some(handler);
        self
    }

    /// Builder method to dynamically define role hierarchies at startup
    pub fn add_role_inheritance(mut self, parent: &str, children: Vec<&str>) -> Self {
        let child_strings = children.into_iter().map(|s| s.to_string()).collect();
        self.role_inheritance
            .insert(parent.to_string(), child_strings);
        self
    }
    
    /// A framework-level diagnostic utility that prints highly optimized operational logs.
    pub fn log_lifecycle(&self, ctx: &RequestContext, status: u16, duration: std::time::Duration) {
        let session_id_log = ctx.session.as_ref().map(|s| s.lock().unwrap().id.clone());
        let jwt_sub_log = ctx.claims.as_ref().map(|c| c.sub.clone());

        log_request_summary(&ctx.req, status, duration, session_id_log, jwt_sub_log);
    }
}
