use crate::{http::request::HttpMethod, routing::engine::Handler};
use super::handler::IntoHandler;
use std::collections::HashMap;

// Globally collected from any file
pub struct AutoRoute {
    pub path: &'static str,
    pub method: HttpMethod,
    pub handler: Handler,
    pub required_role: Option<&'static str>,
    pub request_body_schema: Option<&'static str>,
}

// Tell the compiler to create a tracking registry for AutoRoute elements
inventory::collect!(AutoRoute);

// A unified tracking entry for execution RBAC and security parameters
pub struct RouteTarget {
    pub handler: Box<dyn IntoHandler>,
    pub required_role: Option<&'static str>,
}

pub struct Node {
    pub children: HashMap<String, Node>,
    pub is_end: bool,
    pub methods: HashMap<HttpMethod, RouteTarget>,
    pub parameter_name: Option<String>,
}

impl Node {
    pub fn new() -> Self {
        Node {
            children: HashMap::new(),
            is_end: false,
            methods: HashMap::new(),
            parameter_name: None,
        }
    }
}