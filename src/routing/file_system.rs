use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::http::request::HttpMethod;
use crate::routing::engine::IntoHandler;

pub struct RegisteredFileRoute {
    pub method: HttpMethod,
    // We store a factory function pointer that generates our boxed handler trait object
    pub handler_factory: fn() -> Box<dyn IntoHandler>,
    /// Stateless Role Claim constraint required to unlock this specific file-system endpoint
    pub required_role: Option<&'static str>,
}

pub static FILE_ROUTING_REGISTRY: Lazy<Mutex<HashMap<String, RegisteredFileRoute>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
