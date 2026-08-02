use crate::core::event_bus::{EventRegistration, GritEvent, GritEventHandler};
use crate::core::logger::{self, log_request_summary, LogLevel};
use crate::middleware::{AfterRequestHook, Middleware};
use crate::routing::engine::Router;
use crate::routing::RequestContext;
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

    /// Builder method to dynamically define role hierarchies at startup
    pub fn add_role_inheritance(mut self, parent: &str, children: Vec<&str>) -> Self {
        let child_strings = children.into_iter().map(|s| s.to_string()).collect();
        let map = Arc::make_mut(&mut self.role_inheritance);
        map.insert(parent.to_string(), child_strings);
        self
    }

    /// A framework-level diagnostic utility that prints highly optimized operational logs.
    pub fn log_lifecycle(&self, ctx: &RequestContext, status: u16, duration: std::time::Duration) {
        // Zero-cost check: skip allocation and logging overhead entirely
        if !self.enable_lifecycle_logs {
            return;
        }

        let session_id_log = ctx.session.as_ref().map(|s| s.lock().unwrap().id.clone());
        let jwt_sub_log = ctx.claims.as_ref().map(|c| c.sub.clone());

        log_request_summary(&ctx.req, status, duration, session_id_log, jwt_sub_log);
    }

    /// Register an event handler into the framework's EventBus at boot time
    pub fn register_event_handler<E, H>(self, handler: H) -> Self
    where
        E: GritEvent,
        H: GritEventHandler<E> + 'static,
    {
        self.event_bus.register_handler::<E, H>(handler);
        self
    }

    pub fn auto_discover_handlers(&self) {
        // Automatically reads all #[event_handler] annotations across the entire crate!
        for registration in inventory::iter::<EventRegistration> {
            (registration.register)(&self.event_bus);
        }
    }

    // Registers a job queue storage engine and automatically boots the background worker pool.
    // pub fn with_job_queue(mut self, storage: Arc<dyn JobStorage>, max_workers: usize) -> Self {
    //     // 1. Store storage reference for RequestContext injection
    //     self.job_queue = storage.clone();

    //     // 2. Automatically instantiate and spawn worker engine in background Tokio task
    //     let worker_engine = JobWorkerEngine::new(storage, max_workers);
    //     tokio::spawn(async move {
    //         println!(
    //             " [ENGINE] Background JobWorkerEngine spawned with {} workers...",
    //             max_workers
    //         );
    //         worker_engine.start().await;
    //     });

    //     self
    // }
}
