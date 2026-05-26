pub use futures;
pub use inventory;
pub mod core;
pub mod routing;
pub mod protocol {
    pub mod form;
    pub mod request;
    pub mod response;
}
pub mod render;
pub mod security;
pub mod utils;
pub use ctor;

pub use gritshield_macros::*;

// -----------------------------------------------------------------
// DEPENDENCY ISOLATION HUB
// Expose locked framework dependencies to prevent version conflict mismatch errors.
// -----------------------------------------------------------------
pub mod deps {
    pub use chrono;
    pub use once_cell;
    pub use sea_orm;
    pub use sea_orm_migration;
    pub use serde;
    pub use serde_json;
    pub use tokio;
    pub use uuid;
    pub use tokio_tungstenite;
    pub use futures_util;
}

/// The Prelude module contains everything a developer needs to build an app.
/// Instead of importing 10 different things, just use:
/// use gritshield::prelude::*;
pub mod prelude {
    pub use crate::protocol::form::{FormData, UploadedFile};
    pub use crate::protocol::request::Request;
    pub use crate::protocol::response::Response;
    pub use crate::render;
    pub use crate::routing::templates::TemplateEngine;
    pub use crate::routing::trie::{RequestContext, Router};
    pub use crate::security::xss::Sanitizer;

    // Re-export macros for the prelude
    pub use crate::{delete, get, patch, post, put};

    // External essentials the developer will always need
    pub use maud::{Markup, Render, html};
    pub use std::sync::Arc;

    // Luxury shortcut aliases directly into the prelude so developers don't type deep namespaces
    pub use crate::deps::once_cell::sync::Lazy as OnceLazy;
    pub use crate::deps::uuid::Uuid;
}
