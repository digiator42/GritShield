pub mod shell;
pub mod models;
pub mod repositories;
pub mod metrics_render;
pub mod auth;
pub mod dashboard;

pub use shell::admin_shell;
pub use dashboard::*;
pub use auth::handlers::*;
pub use crate::database::repository::registry::*;