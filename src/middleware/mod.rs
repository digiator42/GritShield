// src/middleware/mod.rs
pub mod utils;
pub mod auth;
pub mod cors;
pub mod logger;
pub mod rate_limit;
pub mod ip_blacklist;

// Re-export utils items
pub use utils::{
    Middleware, MiddlewareResult, MiddlewareState,
    AfterRequestHook,
};

// Re-export middleware implementations
pub use auth::AuthMiddleware;
pub use cors::CorsMiddleware;
pub use logger::LoggerMiddleware;
pub use rate_limit::RateLimitMiddleware;
pub use ip_blacklist::IPBlacklistMiddleware;