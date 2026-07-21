use crate::core::event_bus::{EventBus, JobStorage, JobWorkerEngine, MemoryJobQueue};
use crate::middleware::{AfterRequestHook, Middleware};
use crate::routing::engine::fallback::PageHandlerFn;
use crate::routing::engine::{Node, GLOBAL_FALLBACK};
use crate::routing::IntoHandler;
use crate::security::errors::{default_framework_error_handler, GlobalErrorHandler};
use crate::security::telemetry::SystemTelemetry;
use crate::security::xss::UntrustedString;
use sea_orm::DatabaseConnection;
use std::collections::HashMap;
use std::sync::Arc;

pub enum RoutingResult<'a> {
    Found(
        &'a dyn IntoHandler,
        Option<&'static str>,
        HashMap<String, UntrustedString>,
    ),
    MethodNotAllowed,
    NotFound,
}

pub struct Router {
    pub root: Node,
    pub middlewares: Vec<Box<dyn Middleware>>,
    pub db: Option<Arc<DatabaseConnection>>,
    pub after_hooks: Vec<Box<dyn AfterRequestHook>>,
    pub global_error_handler: GlobalErrorHandler,
    pub telemetry: SystemTelemetry,
    pub event_bus: Arc<EventBus>,
    pub job_queue: Arc<dyn JobStorage>,
    pub fallback_handler: Option<PageHandlerFn>,
    pub role_registry: HashMap<String, &'static str>,
    pub role_inheritance: HashMap<String, Vec<String>>,
}

impl Router {
    pub fn new() -> Self {
        crate::core::logger::init_from_env();

        let fallback = if let Ok(guard) = GLOBAL_FALLBACK.lock() {
            guard.clone()
        } else {
            None
        };

        let mut router = Router {
            root: Node::new(),
            middlewares: Vec::new(),
            db: None,
            after_hooks: Vec::new(),
            global_error_handler: GlobalErrorHandler {
                handler: Some(default_framework_error_handler),
            },
            telemetry: SystemTelemetry::new(),
            event_bus: Arc::new(EventBus::init()),
            job_queue: Arc::new(MemoryJobQueue::new()),
            fallback_handler: fallback,
            role_registry: HashMap::new(),
            role_inheritance: HashMap::new(),
        };

        let worker_engine = JobWorkerEngine::new(router.job_queue.clone(), 10); // 10 concurrent worker slots
        tokio::spawn(async move {
            worker_engine.start().await;
        });

        router.event_bus.auto_discover();

        // Register auto routes
        router.register_auto_routes();

        // Register admin routes
        #[cfg(feature = "admin")]
        router.register_admin_routes();

        // Register swagger routes
        #[cfg(feature = "swagger")]
        router.register_swagger_routes();

        router
    }
}
