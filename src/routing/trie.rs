use crate::protocol::request::{HttpMethod, Request};
use crate::protocol::response::Response;
use crate::security::jwt::Claims;
use crate::security::middleware::{Middleware, MiddlewareResult};
use crate::security::xss::{SafeHtml, UntrustedString};
use std::collections::HashMap;
use std::hash::Hash;

pub type Handler = fn(RequestContext) -> Response;
pub struct RequestContext {
    pub params: HashMap<String, UntrustedString>,
    pub headers: HashMap<String, String>,
    pub claims: Option<Claims>,
    pub query: HashMap<String, UntrustedString>,
}

// impl RequestContext {
//     pub fn new() -> Self {
//         Self {
//             params: HashMap::new(),
//         }
//     }
// }

pub struct Node {
    pub children: HashMap<String, Node>,
    pub is_end: bool,
    pub methods: HashMap<HttpMethod, Handler>,
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

pub enum RoutingResult {
    Found(Handler, HashMap<String, UntrustedString>),
    MethodNotAllowed,
    NotFound,
}

pub struct Router {
    root: Node,
    middlewares: Vec<Box<dyn Middleware>>, // A list of dynamic trait objects
}

impl Router {
    pub fn new() -> Self {
        Router {
            root: Node::new(),
            middlewares: Vec::new(),
        }
    }

    pub fn add_middleware<M: Middleware + 'static>(&mut self, middleware: M) {
        self.middlewares.push(Box::new(middleware));
    }

    pub fn run_middlewares(&self, req: &Request) -> MiddlewareResult {
        for middleware in &self.middlewares {
            match middleware.execute(req) {
                MiddlewareResult::Next => continue,
                MiddlewareResult::Error(res) => return MiddlewareResult::Error(res),
            }
        }
        MiddlewareResult::Next
    }

    pub fn add_route(&mut self, method: HttpMethod, path: &str, handler: Handler) {
        let mut current = &mut self.root;

        for segment in path.split('/').filter(|s| !s.is_empty()) {
            if segment.starts_with(':') {
                let param_name = segment[1..].to_string();
                current = current
                    .children
                    .entry(":param".to_string())
                    .or_insert(Node::new());
                current.parameter_name = Some(param_name);
            } else {
                current = current
                    .children
                    .entry(segment.to_string())
                    .or_insert(Node::new());
            }
        }
        current.is_end = true;
        current.methods.insert(method, handler);
    }

    pub fn match_route(&self, method: &HttpMethod, path: &str) -> RoutingResult {
        let mut current = &self.root;
        let mut params = HashMap::new();

        for segment in path.split('/').filter(|s| !s.is_empty()) {
            if let Some(next_node) = current.children.get(segment) {
                current = next_node;
            } else if let Some(param_node) = current.children.get(":param") {
                if let Some(ref name) = param_node.parameter_name {
                    // If the segment isn't a direct match, check if this level accepts a parameter
                    params.insert(name.clone(), UntrustedString::new(segment.to_string()));
                }
                current = param_node;
            } else {
                return RoutingResult::NotFound;
            }
        }
        // Check if the specific method is supported at this node
        match current.methods.get(method) {
            Some(handler) => RoutingResult::Found(*handler, params),
            None => {
                if !current.methods.is_empty() {
                    RoutingResult::MethodNotAllowed
                } else {
                    RoutingResult::NotFound
                }
            }
        }
    }
}
