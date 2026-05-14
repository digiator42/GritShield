use futures::future::BoxFuture;
use sea_orm::DatabaseConnection;

use crate::protocol::form::FormData;
use crate::protocol::request::{HttpMethod, Request};
use crate::protocol::response::Response;
use crate::security::jwt::Claims;
use crate::security::middleware::{Middleware, MiddlewareResult};
use crate::security::session::{Session, SessionStore};
use crate::security::xss::{SafeHtml, UntrustedString};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub type Handler = fn(RequestContext) -> BoxFuture<'static, Response>;
pub struct RequestContext {
    pub params: HashMap<String, UntrustedString>,
    pub headers: HashMap<String, String>,
    pub claims: Option<Claims>,
    pub query: HashMap<String, UntrustedString>,
    pub session: Option<Arc<Mutex<Session>>>,
    pub form: FormData,
    pub db: Arc<DatabaseConnection>,
}

impl RequestContext {
    pub fn start_session(store: &SessionStore) -> Arc<Mutex<Session>> {
        let (ptr, _) = store.get_or_create(None);
        ptr
    }
}

// The struct that will be globally collected from any file
pub struct AutoRoute {
    pub path: &'static str,
    pub method: HttpMethod,
    pub handler: Handler,
}

// Tell the compiler to create a tracking registry for AutoRoute elements
inventory::collect!(AutoRoute);

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
        let mut router = Router {
            root: Node::new(),
            middlewares: Vec::new(),
        };

        for route in inventory::iter::<AutoRoute> {
            println!(
                "[AUTO-DISCOVERY] Registering {} {:?}",
                route.path, route.method
            );
            router.add_route(route.method, route.path, route.handler);
        }

        router
    }

    pub fn mount(&mut self, route_info: (&str, HttpMethod, Handler)) {
        self.add_route(route_info.1, route_info.0, route_info.2);
    }

    pub fn add_middleware<M: Middleware + 'static>(&mut self, middleware: M) {
        self.middlewares.push(Box::new(middleware));
    }

    pub fn run_middlewares(&self, req: &Request) -> MiddlewareResult {
        let mut session_data = None;

        for middleware in &self.middlewares {
            match middleware.execute(req) {
                MiddlewareResult::Next(state) => {
                    if state.is_some() {
                        session_data = state; // Persist state across chain
                    }
                    continue;
                }
                MiddlewareResult::Error(res) => return MiddlewareResult::Error(res),
            }
        }
        MiddlewareResult::Next(session_data)
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
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        for (i, segment) in segments.iter().enumerate() {
            if let Some(next_node) = current.children.get(*segment) {
                current = next_node;
            } else if let Some(param_node) = current.children.get(":param") {
                if let Some(ref name) = param_node.parameter_name {
                    // Check if this is a wildcard parameter (e.g., *path)
                    if name.starts_with('*') {
                        // Grab all remaining segments joined by slashes
                        let remainder = segments[i..].join("/");
                        params.insert(name.clone(), UntrustedString::new(remainder));
                        current = param_node;
                        break; // Exit loop, we've consumed everything
                    } else {
                        params.insert(name.clone(), UntrustedString::new(segment.to_string()));
                    }
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
