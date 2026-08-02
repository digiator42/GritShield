use crate::core::event_bus::{EventBus, JobStorage, MemoryJobQueue};
use crate::core::{AutoWire, get_env, init_from_env};
use crate::middleware::{AfterRequestHook, Middleware};
use crate::routing::engine::Node;
use crate::routing::IntoHandler;
use crate::security::errors::{default_framework_error_handler, GlobalErrorHandler};
use crate::security::telemetry::SystemTelemetry;
use crate::security::xss::UntrustedString;
use sea_orm::DatabaseConnection;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

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
    pub role_registry: HashMap<String, &'static str>,
    pub role_inheritance: Arc<HashMap<String, Vec<String>>>,
    pub enable_lifecycle_logs: bool,
    pub secret_key: String,
}

impl Router {
    pub fn new() -> Self {
        let enable_lifecycle_logs = init_from_env();

        AutoWire::boot_di_container();

        let secret_key = get_env("JWT_SECRET", &Uuid::new_v4().to_string());

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
            role_registry: HashMap::new(),
            role_inheritance: Arc::new(HashMap::new()),
            enable_lifecycle_logs,
            secret_key,
        };

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

    pub fn debug_dump_tree(&self) {
        println!("\n========= [ROUTER TRIE DUMP] =========");
        self.root.dump_node(0);
        println!("=====================================\n");
    }
}
