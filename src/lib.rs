pub use futures;
pub use inventory;
pub mod core;
pub mod routing;
pub mod protocol {
    pub mod form;
    pub mod request;
    pub mod response;
}
pub mod migration;
pub mod model;
pub mod render;
pub mod security;
pub mod templates;
pub mod utils;
pub use ctor;

pub use gritshield_macros::*;

/// The Prelude module contains everything a developer needs to build an app.
/// Instead of importing 10 different things, just use:
/// use gritshield::prelude::*;
pub mod prelude {
    pub use crate::protocol::form::{FormData, UploadedFile};
    pub use crate::protocol::request::Request;
    pub use crate::protocol::response::Response;
    pub use crate::render;
    pub use crate::routing::trie::{RequestContext, Router};
    pub use crate::security::xss::Sanitizer;

    // Re-export macros for the prelude
    pub use crate::{delete, get, patch, post, put};

    // External essentials the developer will always need
    pub use maud::{html, Markup, Render};
    pub use sea_orm::DatabaseConnection;
    pub use std::sync::Arc;
}
