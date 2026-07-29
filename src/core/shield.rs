use crate::{
    core::{get_env, initialize_env, LogLevel},
    http::ignite,
    middleware::Middleware,
    routing::Router,
};
use sea_orm::DatabaseConnection;
use std::sync::Arc;

/// The main entry point for GritShield applications.
/// Provides a clean, fluent API for configuring and launching the framework.
#[derive(Default)]
pub struct Shield {
    router: Option<Router>,
    host: String,
    port: String,
    log_level: Option<LogLevel>,
    db: Option<Arc<DatabaseConnection>>,
    middlewares: Vec<Box<dyn Middleware>>,
}

impl Shield {
    /// Create a new Shield instance with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the Shield configuration (loads .env and applies defaults)
    pub fn build() -> Self {
        let mut shield = Self::new();

        // Load .env file if it exists
        initialize_env();

        // Read host and port from environment if available
        let host = get_env("SHIELD_HOST", "127.0.0.1");
        let port = get_env("SHIELD_PORT", "8080");

        shield.host = host;
        shield.port = port;

        // Read log level from environment if available
        if let Some(level) = LogLevel::from_str(&get_env("SHIELD_LOG", "")) {
            shield.log_level = Some(level);
        }

        shield
    }

    /// Set the host address
    pub fn host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Set the port
    pub fn port(mut self, port: impl Into<String>) -> Self {
        self.port = port.into();
        self
    }

    /// Set the log level
    pub fn log_level(mut self, level: LogLevel) -> Self {
        self.log_level = Some(level);
        self
    }

    /// Mount a database connection
    pub fn mount_db(mut self, db: Arc<DatabaseConnection>) -> Self {
        self.db = Some(db);
        self
    }

    /// Add a middleware to the pipeline
    pub fn add_middleware(mut self, middleware: impl Middleware + 'static) -> Self {
        self.middlewares.push(Box::new(middleware));
        self
    }

    /// Use a custom router instead of the default
    pub fn router(mut self, router: Router) -> Self {
        self.router = Some(router);
        self
    }

    /// Configure with default settings and launch
    pub fn launch(self) {
        self.launch_with_defaults("127.0.0.1", "8080")
    }

    /// Configure with custom host and port and launch
    pub fn launch_with(self, host: &str, port: &str) {
        self.launch_with_defaults(host, port)
    }

    fn launch_with_defaults(self, host: &str, port: &str) {
        // Build router
        let router = self.router.unwrap_or_else(|| {
            let mut r = Router::new();
            if let Some(level) = self.log_level {
                r = r.mount_logger(level);
            }
            if let Some(db) = self.db {
                r = r.mount_db(db);
            }
            for mw in self.middlewares {
                r = r.add_middleware(mw);
            }
            r
        });

        let final_host = if self.host.is_empty() {
            host
        } else {
            &self.host
        };
        let final_port = if self.port.is_empty() {
            port
        } else {
            &self.port
        };

        // Launch using the framework's runtime
        if let Ok(_handle) = tokio::runtime::Handle::try_current() {
            // Runtime already exists (e.g., from #[launch] macro)
            // Use the existing runtime
            tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    ignite(final_host, final_port, router).await;
                });
            });
        } else {
            // No runtime exists – create one
            let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
            runtime.block_on(async {
                ignite(final_host, final_port, router).await;
            });
        }
    }

    /// Launch with custom host and port (shorthand)
    pub fn launch_at(self, host: &str, port: &str) {
        self.launch_with_defaults(host, port)
    }
}

// Also provide a builder-style `GritShield` alias for clarity
pub type GritShield = Shield;
