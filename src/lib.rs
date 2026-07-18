pub mod core;
pub mod database;
pub mod http;
pub mod macros;
pub mod middleware;
pub mod routing;
pub mod security;
pub mod utils;

pub use ctor;
pub use futures;
pub use inventory;

#[cfg(feature = "admin")]
pub mod gritadmin;

#[cfg(feature = "admin")]
pub use gritadmin::shell::admin_shell;

pub use gritshield_macros::action;
pub use gritshield_macros::component;
pub use gritshield_macros::controller;
#[cfg(feature = "admin")]
pub use gritshield_macros::GritAdmin;
pub use gritshield_macros::GritComponent;
pub use gritshield_macros::GritWire;
pub use gritshield_macros::GritModel;
pub use gritshield_macros::GritRelation;
#[cfg(feature = "swagger")]
pub use gritshield_macros::GritSchema;
pub use gritshield_macros::WireContainer;
pub use gritshield_macros::{delete, get, patch, post, put};

// explicit module namespace for startup macros
pub mod startup {
    pub use ctor::ctor;
}

// -----------------------------------------------------------------
// DEPENDENCY ISOLATION HUB
// Expose locked framework dependencies to prevent version conflict mismatch errors.
// -----------------------------------------------------------------
pub mod deps {
    pub use chrono;
    pub use futures;
    pub use futures_util;
    pub use once_cell;
    pub use rust_decimal;
    pub use rust_decimal::Decimal;
    pub use sea_orm;
    pub use sea_orm_migration;
    pub use sea_orm_migration::async_trait::async_trait;
    pub use serde;
    pub use serde_json;
    pub use tokio;
    pub use tokio_tungstenite;
    pub use uuid;
}

/// The Prelude module contains everything you need to build an app.
/// Instead of importing 10 different things, just use:
/// use gritshield::prelude::*;
pub mod prelude {
    pub use crate::http::form::{FormData, UploadedFile};
    pub use crate::http::request::Request;
    pub use crate::http::response::Response;
    pub use crate::http::HttpMethod;
    pub use crate::macros;
    pub use crate::routing::engine::{RequestContext, Router};
    pub use crate::routing::templates::TemplateEngine;
    pub use crate::security::xss::Sanitizer;

    // Critical functions
    pub use crate::core::env::get_env;
    pub use crate::http::ignite;

    // Re-export macros for the prelude
    pub use crate::action;
    pub use crate::controller;
    #[cfg(feature = "admin")]
    pub use crate::GritAdmin;
    pub use crate::GritModel;
    pub use crate::GritRelation;
    #[cfg(feature = "swagger")]
    pub use crate::GritSchema;
    pub use crate::{delete, get, patch, post, put};

    // External essentials the developer will always need
    pub use maud::{html, Markup, Render};
    pub use std::sync::Arc;

    // Luxury shortcut aliases directly into the prelude so developers don't type deep namespaces
    pub use crate::deps::once_cell::sync::Lazy as OnceLazy;
    pub use crate::deps::uuid::Uuid;
}
