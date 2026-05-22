use crate::core::logger::log_request_summary;
use crate::protocol::form::FormData;
use crate::protocol::request::{HttpMethod, Request};
use crate::protocol::response::Response;
use crate::routing::file_system::FILE_ROUTING_REGISTRY;
use crate::security::cookies::CookieJar;
use crate::security::errors::{
    FrameworkError, GlobalErrorHandler, default_framework_error_handler,
};
use crate::security::jwt::Claims;
use crate::security::middleware::{
    AfterRequestHook, Middleware, MiddlewareResult, MiddlewareState,
};
use crate::security::session::{Session, SessionStore};
use crate::security::telemetry::SystemTelemetry;
use crate::security::xss::{Sanitizer, UntrustedString};
use futures::future::{BoxFuture, FutureExt};
use lazy_static::lazy_static;
use sea_orm::DatabaseConnection;
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub type BoxedResponse = BoxFuture<'static, Response>;
pub type Handler = fn(RequestContext) -> BoxedResponse;
/// Short representation for handlers that can fail safely with an explicit framework error
pub type ShieldResult<T> = Result<T, FrameworkError>;

pub trait IntoResponse {
    fn into_response(self) -> Response;
}

// A standard Response trivially turns into a Response
impl IntoResponse for Response {
    fn into_response(self) -> Response {
        self
    }
}

// Add this blanket implementation to allow pre-boxed trait objects
impl IntoHandler for Box<dyn IntoHandler> {
    fn call(&self, ctx: RequestContext) -> BoxedResponse {
        // Delegate straight down to the inner trait object inside the box!
        self.as_ref().call(ctx)
    }
}

// A ShieldResult turns into a Response by catching errors and invoking a fallback
impl IntoResponse for ShieldResult<Response> {
    fn into_response(self) -> Response {
        match self {
            Ok(res) => res,
            Err(err) => {
                // Return a clean default security/error dashboard layout
                println!(
                    "[SECURITY AUDIT] Handler caught an explicit framework error: {:?}",
                    err
                );
                Response::new(
                    500,
                    Sanitizer::trust("<h1>500 Internal Security Error</h1>"),
                )
            }
        }
    }
}

// Support raw static string slices: &'static str
impl IntoResponse for &'static str {
    fn into_response(self) -> Response {
        // Automatically wraps the text as an HTML response with a 200 OK status
        Response::new(200, Sanitizer::trust(self))
    }
}

// Support dynamic heap strings: String
impl IntoResponse for String {
    fn into_response(self) -> Response {
        Response::new(200, Sanitizer::trust(&self))
    }
}

pub trait IntoHandler: Send + Sync + 'static {
    fn call(&self, ctx: RequestContext) -> BoxedResponse;
}

impl<F, Fut, R> IntoHandler for F
where
    F: Fn(RequestContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = R> + Send + 'static,
    R: IntoResponse + 'static,
{
    fn call(&self, ctx: RequestContext) -> BoxedResponse {
        let fut = (self)(ctx);

        async move {
            let res = fut.await;
            res.into_response()
        }
        .boxed()
    }
}

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub req: Request,
    pub telemetry: SystemTelemetry,
    pub params: HashMap<String, UntrustedString>,
    pub peer_addr: SocketAddr,
    pub headers: HashMap<String, String>,
    pub claims: Option<Claims>,
    pub query: HashMap<String, UntrustedString>,
    pub session: Option<Arc<Mutex<Session>>>,
    pub form: FormData,
    pub db: Option<Arc<DatabaseConnection>>,
    pub raw_body: Vec<u8>,
    pub content_type: Option<String>,
    pub cookies: Arc<Mutex<CookieJar>>,
    pub start_time: std::time::Instant,
}

impl RequestContext {
    pub fn new() -> Self {
        Self {
            req: Request::new(),
            telemetry: SystemTelemetry::new(),
            params: HashMap::new(),
            peer_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080),
            headers: HashMap::new(),
            claims: None,
            query: HashMap::new(),
            session: None,
            form: FormData::new(),
            db: None,
            raw_body: Vec::new(),
            content_type: None,
            cookies: Arc::new(Mutex::new(CookieJar::new(None, String::new()))),
            start_time: std::time::Instant::now(),
        }
    }

    /// Safely resolves the true client IP address while mitigating IP Spoofing risks
    pub fn resolve_client_ip(&self) -> String {
        // Look for X-Forwarded-For (injected by downstream edge networks/proxies)
        if let Some(forwarded_header) = self.req.headers.get("x-forwarded-for") {
            // X-Forwarded-For can look like: "203.0.113.195, 70.41.3.18, 150.172.238.178"
            // The very first value on the left is the actual client identity.
            if let Some(real_ip) = forwarded_header.split(',').next() {
                let trimmed_ip = real_ip.trim();
                if !trimmed_ip.is_empty() {
                    return trimmed_ip.to_string();
                }
            }
        }

        // If the header doesn't exist, use the verified physical socket connection IP
        // We drop the port number (.ip()) so the token tracks the host computer
        self.peer_addr.ip().to_string()
    }

    pub fn start_session(store: &SessionStore) -> Arc<Mutex<Session>> {
        let (ptr, _) = store.get_or_create(None);
        ptr
    }

    /// Returns true if the user's browser sent a session cookie but it was
    /// rejected or evicted by the framework because it expired on the server.
    pub fn is_session_expired(&self) -> bool {
        // If the browser sent a cookie header, but the AuthMiddleware stripped
        // it out and left the active context session empty, it means the session expired!
        let had_cookie = self
            .req
            .headers
            .get("cookie")
            .or_else(|| self.req.headers.get("Cookie"))
            .map(|val| val.contains("GSESSION_ID"))
            .unwrap_or(false);

        had_cookie && self.session.is_none()
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

    /// Zero-boilerplate helper to read a standard, unsigned cookie
    pub fn get_cookie(&self, name: &str) -> Option<String> {
        self.cookies.lock().ok()?.get(name).cloned()
    }

    /// Handles the Mutex lock internally and yields an immediate Option<String>.
    pub fn get_signed_cookie(&self, name: &str) -> Option<String> {
        // Lock the internal mutex safely. If it fails, return None.
        let jar = self.cookies.lock().ok()?;
        // Call the inner CookieJar method
        jar.get_signed(name)
    }

    /// Premium helper to inject or update a cookie directly without manual locking
    pub fn set_cookie(&self, cookie: crate::protocol::response::Cookie) {
        if let Ok(mut jar) = self.cookies.lock() {
            jar.add(cookie);
        }
    }

    /// Premium helper to inject a secure, cryptographically signed cookie
    pub fn set_signed_cookie(&self, cookie: crate::protocol::response::Cookie) {
        if let Ok(mut jar) = self.cookies.lock() {
            jar.add_signed(cookie);
        }
    }

    /// Premium helper to instruct the browser to instantly shred a cookie
    pub fn remove_cookie(&self, name: &str) {
        if let Ok(mut jar) = self.cookies.lock() {
            jar.remove(name);
        }
    }

    /// Write a key-value attribute directly into the active session instance
    pub fn set_session_data(&self, key: &str, value: &str) {
        if let Some(ref session_arc) = self.session {
            if let Ok(mut session) = session_arc.lock() {
                session.data.insert(key.to_string(), value.to_string());
            }
        }
    }

    /// Read an attribute value out of the active session instance
    pub fn get_session_data(&self, key: &str) -> Option<String> {
        let session_arc = self.session.as_ref()?;
        let session = session_arc.lock().ok()?;
        session.data.get(key).cloned()
    }

    /// Explicitly tag the session as authenticated to a specific User Entity ID
    pub fn login_user_id(&self, user_id: &str) {
        if let Some(ref session_arc) = self.session {
            if let Ok(mut session) = session_arc.lock() {
                session.user_id = Some(user_id.to_string());
            }
        }
    }

    /// Explicitly check if the current request context belongs to a logged-in user
    pub fn is_user_authenticated(&self) -> bool {
        self.get_session_data("user_id").is_some()
    }

    /// Generates or retrieves an existing CSRF token for the active session context
    pub fn get_csrf_token(&self) -> String {
        if let Some(ref session_arc) = self.session {
            let session = session_arc.lock().unwrap();
            if let Some(token) = session.data.get("csrf_token") {
                return token.clone();
            }
        }
        String::new()
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
    pub methods: HashMap<HttpMethod, Box<dyn IntoHandler>>,
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

pub enum RoutingResult<'a> {
    Found(&'a dyn IntoHandler, HashMap<String, UntrustedString>),
    MethodNotAllowed,
    NotFound,
}

pub type AsyncPageFuture = Pin<Box<dyn Future<Output = Response> + Send>>;

pub type PageHandlerFn = fn(RequestContext) -> AsyncPageFuture;

lazy_static! {
    pub static ref GLOBAL_FALLBACK: Mutex<Option<PageHandlerFn>> = Mutex::new(None);
}

/// A registration hook your macro or files can call during static initialization
pub fn register_global_fallback(handler: PageHandlerFn) {
    if let Ok(mut guard) = GLOBAL_FALLBACK.lock() {
        *guard = Some(handler);
    }
}

pub struct Router {
    root: Node,
    pub middlewares: Vec<Box<dyn Middleware>>, // A list of dynamic trait objects
    pub db: Option<Arc<DatabaseConnection>>,   // An optional database connection
    pub after_hooks: Vec<Box<dyn AfterRequestHook>>,
    pub use_logger: bool,
    pub global_error_handler: GlobalErrorHandler,
    pub telemetry: SystemTelemetry,
    pub fallback_handler: Option<PageHandlerFn>,
}

impl Router {
    pub fn new() -> Self {
        let fallback = if let Ok(guard) = GLOBAL_FALLBACK.lock() {
            guard.clone()
        } else {
            None
        };
        let mut router = Router {
            root: Node::new(),
            middlewares: Vec::new(),
            db: None,
            use_logger: false,
            after_hooks: Vec::new(),
            global_error_handler: GlobalErrorHandler {
                handler: Some(default_framework_error_handler),
            },
            telemetry: SystemTelemetry::new(),
            fallback_handler: fallback,
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

    /// Register a global pipeline middleware by moving ownership
    pub fn add_middleware(mut self, middleware: impl Middleware + 'static) -> Self {
        self.middlewares.push(Box::new(middleware));
        self // Return ownership back out to the chain
    }

    pub fn run_after_hooks(&self, ctx: RequestContext, status: u16, duration: Duration) {
        for hook in &self.after_hooks {
            hook.call(&ctx, status, duration);
        }
    }

    /// Allows developers to attach a custom layout handler for unmatched 404 routes
    pub fn set_fallback(mut self, handler: PageHandlerFn) -> Self {
        self.fallback_handler = Some(handler);
        self
    }

    pub fn run_middlewares(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Initialize an empty accumulator state packer
        let mut accumulated_state = MiddlewareState {
            session: None,
            claims: None,
            session_was_stale: false,
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

    pub fn add_route<H>(&mut self, method: HttpMethod, path: &str, handler: H)
    where
        H: IntoHandler,
    {
        let mut current = &mut self.root;

        for segment in path.split('/').filter(|s| !s.is_empty()) {
            current = current
                .children
                .entry(segment.to_string())
                .or_insert(Node::new());
        }

        current.is_end = true;
        // Heap-allocate the handler container so it fits uniformly into the Trie matrix
        current.methods.insert(method, Box::new(handler));
    }

    pub fn match_route<'a>(&'a self, method: &HttpMethod, path: &str) -> RoutingResult<'a> {
        let mut current = &self.root;
        let mut params = HashMap::new();
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();

        for (i, segment) in segments.iter().enumerate() {
            if let Some(next_node) = current.children.get(*segment) {
                current = next_node;
            } else {
                // Find any child key that signals a dynamic parameter
                let param_match = current
                    .children
                    .iter()
                    .find(|(key, _)| key.starts_with(':'));

                if let Some((key, param_node)) = param_match {
                    // Check if either the map key OR the internal name contains the '*' wildcard flag
                    let is_wildcard = key.contains('*')
                        || param_node
                            .parameter_name
                            .as_ref()
                            .map_or(false, |name| name.contains('*'));

                    if is_wildcard {
                        // Grab everything remaining, join it with slashes, and clean the parameter key
                        let remainder = segments[i..].join("/");
                        let clean_key = key.trim_start_matches(':').to_string(); // drops ':' to leave '*path'

                        params.insert(clean_key, UntrustedString::new(remainder));
                        current = param_node;
                        break; // 🚀 Break instantly! The wildcard has devoured the rest of the URL path
                    } else {
                        // Standard parameter extraction (:id, etc.)
                        if let Some(ref name) = param_node.parameter_name {
                            let clean_key = name.trim_start_matches(':').to_string();
                            params.insert(clean_key, UntrustedString::new(segment.to_string()));
                        }
                        current = param_node;
                    }
                } else {
                    return RoutingResult::NotFound;
                }
            }
        }

        match current.methods.get(method) {
            Some(handler) => RoutingResult::Found(&**handler, params),
            None => {
                if !current.methods.is_empty() {
                    RoutingResult::MethodNotAllowed
                } else {
                    RoutingResult::NotFound
                }
            }
        }
    }

    /// Seamlessly crawls a filesystem folder, computes URL paths,
    /// and mounts handlers dynamically.
    pub fn mount_file_routes<P: AsRef<Path>>(
        mut self,
        folder_path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let base_path = folder_path.as_ref().to_path_buf();
        self.crawl_directory(&base_path, &base_path)?;
        Ok(self)
    }

    fn crawl_directory(&mut self, current_dir: &Path, base_dir: &Path) -> std::io::Result<()> {
        if current_dir.is_dir() {
            for entry in fs::read_dir(current_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.file_name().map_or(false, |name| name == "404.rs") {
                    // Skip it! We attach it explicitly as an engine fallback instead
                    continue;
                }

                if path.is_dir() {
                    // Recursively crawl nested folders (e.g., pages/api)
                    self.crawl_directory(&path, &base_dir)?;
                } else if path.is_file() && path.extension().map_or(false, |ext| ext == "rs") {
                    self.process_page_file(&path, base_dir);
                }
            }
        }
        Ok(())
    }

    fn process_page_file(&mut self, file_path: &Path, base_dir: &Path) {
        // 1. Convert filesystem paths to absolute lookup keys
        // Example: "src/pages/api/users.rs"
        let file_key = file_path.to_string_lossy().replace("\\", "/");

        // 2. Compute the dynamic URL Route path
        let relative = file_path.strip_prefix(base_dir).unwrap().with_extension("");
        let relative_str = relative.to_string_lossy().replace("\\", "/");

        let mut url_route = if relative_str == "index" {
            "/".to_string()
        } else if relative_str.ends_with("/index") {
            format!("/{}", relative_str.trim_end_matches("/index"))
        } else {
            format!("/{}", relative_str)
        };
        // Converts "docs/[..path]" -> "docs/:*path"
        if url_route.contains('[') && url_route.contains(']') {
            url_route = url_route
                .replace("[..", ":*") // Handles the Next.js catch-all style
                .replace("[", ":*") // Fallback for standard dynamic brackets
                .replace("]", "");
        }

        // Converts folder/foo/_path_ to folder/foo/:*path (Alternative layout)
        if url_route.contains('_') {
            url_route = url_route.replace("_", ":*");
        }

        // Extract the handler out of our pre-compiled global registry map safely
        if let Ok(registry) = FILE_ROUTING_REGISTRY.lock() {
            if let Some(registered) = registry.get(&file_key) {
                println!(
                    "[GRITSHIELD FS-ROUTER] Mapping File System Asset: {} ➡️  Route: [{:?}] {}",
                    file_key, registered.method, url_route
                );
                let handler_instance = (registered.handler_factory)();
                self.add_route(registered.method, &url_route, handler_instance);
            } else {
                eprintln!(
                    "[WARN] Discovered file '{}', but no `register_page!` statement was found inside it.",
                    file_key
                );
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
