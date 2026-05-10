use crate::protocol::request::HttpMethod;
use crate::security::xss::{SafeHtml, UntrustedString};
use std::collections::HashMap;

pub type Handler = fn(HashMap<String, UntrustedString>) -> SafeHtml;

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
}

impl Router {
    pub fn new() -> Self {
        Router { root: Node::new() }
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
            }

            current = current
                .children
                .entry(segment.to_string())
                .or_insert(Node::new());
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
