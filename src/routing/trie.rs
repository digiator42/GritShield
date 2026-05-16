use futures::future::BoxFuture;
use sea_orm::DatabaseConnection;

use crate::core::logger::log_request_summary;
use crate::protocol::form::FormData;
use crate::protocol::request::{HttpMethod, Request};
use crate::protocol::response::Response;
use crate::security::jwt::Claims;
use crate::security::middleware::{
    AfterRequestHook, Middleware, MiddlewareResult, MiddlewareState,
};
use crate::security::session::{Session, SessionStore};
use crate::security::xss::{SafeHtml, UntrustedString};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type Handler = fn(RequestContext) -> BoxFuture<'static, Response>;

#[derive(Clone)]
pub struct RequestContext {
    pub req: Request,
    pub params: HashMap<String, UntrustedString>,
    pub headers: HashMap<String, String>,
    pub claims: Option<Claims>,
    pub query: HashMap<String, UntrustedString>,
    pub session: Option<Arc<Mutex<Session>>>,
    pub form: FormData,
    pub db: Option<Arc<DatabaseConnection>>,
    pub raw_body: Vec<u8>,
    pub content_type: Option<String>,
    pub start_time: std::time::Instant,
}

impl RequestContext {
    pub fn start_session(store: &SessionStore) -> Arc<Mutex<Session>> {
        let (ptr, _) = store.get_or_create(None);
        ptr
    }

    /// A helper method allowing handlers to cleanly extract JSON data structures
    pub fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        let content_type = self.content_type.as_deref().unwrap_or("");
        if !content_type.starts_with("application/json") {
            return Err("Content-Type must be application/json".to_string());
        }
        serde_json::from_slice(&self.raw_body)
            .map_err(|e| format!("Failed to parse JSON body: {}", e))
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
    pub middlewares: Vec<Box<dyn Middleware>>, // A list of dynamic trait objects
    pub db: Option<Arc<DatabaseConnection>>,   // An optional database connection
    pub after_hooks: Vec<Box<dyn AfterRequestHook>>,
    pub use_logger: bool,
}

impl Router {
    pub fn new() -> Self {
        let mut router = Router {
            root: Node::new(),
            middlewares: Vec::new(),
            db: None,
            use_logger: false,
            after_hooks: Vec::new(),
        };

        for route in inventory::iter::<AutoRoute> {
            println!(
                "[AUTO-ROUTING] Registering {} {:?}",
                route.path, route.method
            );
            router.add_route(route.method, route.path, route.handler);
        }

        router
    }

    pub fn mound_db(mut self, db: Arc<DatabaseConnection>) -> Self {
        self.db = Some(db);
        self
    }

    /// Premium builder to switch on detailed diagnostic server logs
    pub fn mount_logger(mut self) -> Self {
        self.use_logger = true;
        self
    }

    pub fn mount(&mut self, route_info: (&str, HttpMethod, Handler)) {
        self.add_route(route_info.1, route_info.0, route_info.2);
    }

    pub fn add_middleware<M: Middleware + 'static>(&mut self, middleware: M) -> &Self {
        self.middlewares.push(Box::new(middleware));
        self
    }

    pub fn run_after_hooks(&self, ctx: RequestContext, status: u16, duration: Duration) {
        for hook in &self.after_hooks {
            hook.call(&ctx, status, duration);
        }
    }

    pub fn run_middlewares(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Initialize an empty accumulator state packer
        let mut accumulated_state = MiddlewareState {
            session: None,
            claims: None,
        };

        for middleware in &self.middlewares {
            match middleware.execute(ctx) {
                MiddlewareResult::Next(maybe_state) => {
                    if let Some(state) = maybe_state {
                        // Merge fields dynamically without overwriting existing ones with None
                        if state.session.is_some() {
                            accumulated_state.session = state.session;
                        }
                        if state.claims.is_some() {
                            accumulated_state.claims = state.claims;
                        }
                    }
                    continue;
                }
                MiddlewareResult::Error(res) => return MiddlewareResult::Error(res),
            }
        }

        // Return the perfectly merged collection of sessions and claims
        MiddlewareResult::Next(Some(accumulated_state))
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

    /// Builder to mount custom post-execution lifecycle hooks
    pub fn add_after_hook(mut self, hook: Box<dyn AfterRequestHook>) -> Self {
        self.after_hooks.push(hook);
        self
    }

    /// A framework-level diagnostic utility that prints highly optimized operational logs.
    pub fn log_lifecycle(&self, ctx: &RequestContext, status: u16, duration: std::time::Duration) {
        let session_id_log = ctx.session.as_ref().map(|s| s.lock().unwrap().id.clone());
        let jwt_sub_log = ctx.claims.as_ref().map(|c| c.sub.clone());

        log_request_summary(&ctx.req, status, duration, session_id_log, jwt_sub_log);
    }
}
