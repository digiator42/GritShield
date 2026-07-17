pub mod context;
pub mod handler;
pub mod builder;
pub mod fallback;
pub mod route;
pub mod router;

// Re-exports
pub use context::RequestContext;
pub use fallback::{register_global_fallback, GLOBAL_FALLBACK};
pub use handler::{BoxedResponse, Handler, IntoHandler, IntoResponse, ShieldResult};
pub use route::{AutoRoute, Node};
pub use router::{Router, RoutingResult};