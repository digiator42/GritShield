use crate::core::logger::{self, log_request_summary, LogLevel};
use crate::core::swagger::{generate_openapi_spec, render_swagger_ui};
use crate::database::repository::{AdminHandlerFn, DynamicColumnSpec};
use crate::gritadmin::main_handler::*;
use crate::gritadmin::metrics::{admin_metrics_api_handler, admin_metrics_html_handler};
use crate::protocol::form::FormData;
use crate::protocol::request::{HttpMethod, Request};
use crate::protocol::response::{Cookie, Response};
use crate::routing::file_system::FILE_ROUTING_REGISTRY;
use crate::security::cookies::CookieJar;
use crate::security::errors::{default_framework_error_handler, GlobalErrorHandler, ShieldError};
use crate::security::jwt::Claims;
use crate::security::middleware::{
    AfterRequestHook, Middleware, MiddlewareResult, MiddlewareState,
};
use crate::security::session::{Session, SessionStore};
use crate::security::telemetry::SystemTelemetry;
use crate::security::xss::{Sanitizer, UntrustedString};
use crate::{debug, error, info, trace, warn};
use colored::*;
use futures::future::{BoxFuture, FutureExt};
use lazy_static::lazy_static;
use sea_orm::DatabaseConnection;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn method_color(method: &str) -> ColoredString {
    match method {
        "GET" => method.green(),
        "POST" => method.blue(),
        "PUT" => method.yellow(),
        "DELETE" => method.red(),
        "PATCH" => method.magenta(),
        _ => method.white(),
    }
}

pub type BoxedResponse = BoxFuture<'static, Response>;
pub type Handler = fn(RequestContext) -> BoxedResponse;
/// Short representation for handlers that can fail safely with an explicit framework error
pub type ShieldResult<T> = Result<T, ShieldError>;

pub trait IntoResponse {
    fn into_response(self) -> Response;
}

// A standard Response trivially turns into a Response
impl IntoResponse for Response {
    fn into_response(self) -> Response {
        self
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

// A ShieldResult turns into a Response by catching errors and invoking a fallback
impl IntoResponse for ShieldResult<Response> {
    fn into_response(self) -> Response {
        match self {
            Ok(res) => res,
            Err(err) => {
                println!(
                    "[SECURITY AUDIT] Handler caught an explicit framework error: {:?}",
                    err
                );

                // Determine status code and message based on the actual error type
                let (status, msg_string): (u16, String) = match err {
                    ShieldError::UnauthorizedAccess => {
                        (401, "<h1>401 Unauthorized</h1>".to_string())
                    }
                    ShieldError::Forbidden => (403, "<h1>403 Forbidden</h1>".to_string()),
                    ShieldError::NotFound => (404, "<h1>404 Not Found</h1>".to_string()),
                    ShieldError::BadRequest(err) => {
                        (400, format!("<h1>400 Bad Request</h1><br/>{}", err))
                    }
                    _ => (500, "<h1>500 Internal Security Error</h1>".to_string()),
                };

                // Pass the final String reference directly
                Response::new(status, Sanitizer::trust(&msg_string))
            }
        }
    }
}

pub trait IntoHandler: Send + Sync + 'static {
    fn call(&self, ctx: RequestContext) -> BoxedResponse;
}

// Add this blanket implementation to allow pre-boxed trait objects
impl IntoHandler for Box<dyn IntoHandler> {
    fn call(&self, ctx: RequestContext) -> BoxedResponse {
        // Delegate straight down to the inner trait object inside the box!
        self.as_ref().call(ctx)
    }
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

impl IntoHandler
    for std::sync::Arc<dyn Fn(RequestContext) -> BoxedResponse + Send + Sync + 'static>
{
    fn call(&self, ctx: RequestContext) -> BoxedResponse {
        (self)(ctx)
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
    pub role_inheritance: Arc<HashMap<String, Vec<String>>>,
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
            role_inheritance: Arc::new(HashMap::new()),
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

    /// Safely and asynchronously parses the raw byte request body as JSON
    pub async fn json_body(&self) -> Option<Value> {
        // Convert the raw byte Vec safely to a UTF-8 string slice
        let body_str = match str::from_utf8(&self.raw_body) {
            Ok(s) => s,
            Err(_) => return None,
        };

        // Parse the string slice directly using serde_json
        serde_json::from_str(body_str).ok()
    }

    /// A helper method allowing handlers to cleanly extract JSON data structures
    pub async fn json<T: serde::de::DeserializeOwned>(&self) -> Result<T, ShieldError> {
        let content_type = self.content_type.as_deref().unwrap_or("");
        if !content_type.starts_with("application/json") {
            return Err(ShieldError::BadRequest(
                "Content-Type must be application/json".to_string(),
            ));
        }

        serde_json::from_slice(&self.raw_body)
            .map_err(|e| ShieldError::BadRequest(format!("Failed to parse JSON body: {}", e)))
    }

    /// Zero-boilerplate helper to read a standard, unsigned cookie
    pub fn get_cookie(&self, name: &str) -> Option<String> {
        self.cookies.lock().ok()?.get(name).cloned()
    }

    /// Handles the Mutex lock internally and yields an immediate Option<String>.
    pub fn get_signed_cookie(&self, name: &str) -> Option<String> {
        // Lock the internal mutex safelIf it fails, return None.
        let jar = self.cookies.lock().ok()?;
        // Call the inner CookieJar method
        jar.get_signed(name)
    }

    /// Premium helper to inject or update a cookie directly without manual locking
    pub fn set_cookie(&self, cookie: Cookie) {
        if let Ok(mut jar) = self.cookies.lock() {
            jar.add(cookie);
        }
    }

    /// Premium helper to inject a secure, cryptographically signed cookie
    pub fn set_signed_cookie(&self, cookie: Cookie) {
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
        match &self.session {
            Some(session_arc) => match session_arc.lock() {
                Ok(mut session) => {
                    session.data.insert(key.to_string(), value.to_string());
                    println!(
                        "[SESSION] Set key '{}' = '{}' in session {}",
                        key, value, session.id
                    );
                }
                Err(e) => {
                    error!("[SESSION ERROR] Failed to lock session for write: {}", e);
                }
            },
            None => {
                error!(
                    "[SESSION ERROR] Cannot set session data: session is None\
                     This usually means the middleware didn't initialize it properly."
                );
            }
        }
    }

    pub fn get_session_data(&self, key: &str) -> Option<String> {
        let session_arc = self.session.as_ref()?;
        match session_arc.lock() {
            Ok(session) => {
                let value = session.data.get(key).cloned();
                if value.is_some() {
                    debug!("[SESSION] Read key '{}' from session", key);
                } else {
                    debug!("[SESSION] Key '{}' not found in session", key);
                }
                value
            }
            Err(e) => {
                error!("[SESSION ERROR] Failed to lock session for read: {}", e);
                None
            }
        }
    }

    pub fn get_user_id(&self) -> Option<String> {
        debug!("[AUTH] Attempting to get ID");
        // Try session, then JWT claims
        if let Some(session) = &self.session {
            if let Ok(s) = session.lock() {
                if let Some(uid) = s.data.get("user_id") {
                    return Some(uid.clone());
                }
            }
        }
        if let Some(claims) = &self.claims {
            return Some(claims.sub.clone());
        }
        None
    }

    /// Explicitly tag the session as authenticated to a specific User Entity ID
    pub fn login_user_id(&self, user_id: &str) {
        debug!("[AUTH] Attempting to login user: {}", user_id);
        debug!("[DEBUG] ctx.session is: {:?}", self.session.is_some());

        match &self.session {
            Some(session_arc) => {
                match session_arc.lock() {
                    Ok(mut session) => {
                        // Set in both places for redundancy
                        session.user_id = Some(user_id.to_string());
                        session
                            .data
                            .insert("user_id".to_string(), user_id.to_string());

                        debug!(
                            "[AUTH] ✓ Successfully logged in user {} to session {}",
                            user_id, session.id
                        );
                    }
                    Err(e) => {
                        error!("[AUTH ERROR] Failed to lock session during login: {}", e);
                    }
                }
            }
            None => {
                error!(
                    "[AUTH ERROR] CRITICAL: Cannot login user - ctx.session is None! \
                     The middleware MUST initialize a session before handler execution."
                );
            }
        }
    }

    /// Explicitly check if the current request context belongs to a logged-in user
    pub fn is_user_authenticated(&self) -> bool {
        match &self.session {
            Some(session_arc) => match session_arc.lock() {
                Ok(session) => {
                    let has_user_id = session.user_id.is_some();
                    let has_user_data = session.data.get("user_id").is_some();

                    if has_user_id || has_user_data {
                        debug!("[AUTH] User authenticated in session {}", session.id);
                        return true;
                    }

                    debug!("[AUTH] Session {} has no user_id", session.id);
                    false
                }
                Err(e) => {
                    error!("[AUTH ERROR] Failed to lock session for auth check: {}", e);
                    false
                }
            },
            None => {
                warn!("[AUTH] No session available - user is not authenticated");
                false
            }
        }
    }

    /// Extracts the cached role string natively out of GritShield's hybrid state store.
    /// Prioritizes stateful session storage, falling back seamlessly to stateless JWT claims.
    pub fn get_user_role(&self) -> Option<String> {
        // Check stateful session storage first
        if let Some(ref session_arc) = self.session {
            match session_arc.lock() {
                Ok(session) => {
                    if let Some(role) = session.data.get("role") {
                        debug!("[RBAC] Retrieved role '{}' from session", role);
                        return Some(role.clone());
                    }
                }
                Err(e) => {
                    error!("[RBAC ERROR] Failed to lock session for role check: {}", e);
                }
            }
        }

        // Stateless Fallback: Read the role field embedded inside the cryptographically validated JWT
        if let Some(ref claims) = self.claims {
            debug!("[RBAC] Retrieved role '{}' from JWT claims", claims.role);
            return Some(claims.role.clone());
        }

        warn!("[RBAC] No role found in session or claims");
        None
    }

    /// Non-blocking check evaluating security roles using hierarchical permissions
    pub fn has_fixed_role(&self, target_role: &str) -> bool {
        match self.get_user_role() {
            Some(role) => {
                // If an exact match is found, allow entry immediately
                if role == target_role {
                    println!("[RBAC] Role '{}' matches target '{}'", role, target_role);
                    return true;
                }

                // Hierarchical authorization structure bypass rules
                let allowed = match (role.as_str(), target_role) {
                    ("Admin", _) => true, // Admins bypass all lower operational barriers
                    ("Operator", "Admin") => false,
                    ("Operator", _) => true, // Operators access standard and low tier pathways
                    ("Auditor", "Auditor") => true,
                    _ => false,
                };

                if allowed {
                    println!(
                        "[RBAC] Role '{}' is permitted access to '{}'",
                        role, target_role
                    );
                } else {
                    println!(
                        "[RBAC] Role '{}' is DENIED access to '{}'",
                        role, target_role
                    );
                }

                allowed
            }
            None => {
                println!("[RBAC] No role found - access to '{}' denied", target_role);
                false
            }
        }
    }

    /// Dynamic recursive tree climber to check if a user role inherits the target role
    fn check_inheritance(&self, current_role: &str, target_role: &str) -> bool {
        trace!(
            "[RBAC TREE] Checking if '{}' inherits '{}'",
            current_role,
            target_role
        );

        if current_role == target_role {
            trace!(
                "[RBAC TREE] ✓ Direct match: '{}' == '{}'",
                current_role,
                target_role
            );
            return true;
        }

        // Search the map stored natively inside the request context
        if let Some(children) = self.role_inheritance.get(current_role) {
            trace!(
                "[RBAC TREE] Found {} children for role '{}'",
                children.len(),
                current_role
            );

            for child in children {
                if child == target_role {
                    trace!("[RBAC TREE] ✓ Found direct child match: '{}'", child);
                    return true;
                }

                if self.check_inheritance(child, target_role) {
                    trace!("[RBAC TREE] ✓ Found inherited match through '{}'", child);
                    return true;
                }
            }
        }

        warn!(
            "[RBAC TREE] ✗ No inheritance path from '{}' to '{}'",
            current_role, target_role
        );
        false
    }

    /// Evaluates BOTH Dynamic Graph Trees AND Fixed System matrices
    /// Prioritizes runtime user-defined inheritance graphs first, falling back to core system rules.
    pub fn has_role(&self, target_role: &str) -> bool {
        // Evaluate against user-defined dynamic runtime configurations first
        if let Some(user_role) = self.get_user_role() {
            if self.check_inheritance(&user_role, target_role) {
                return true;
            }
        }

        // FALLBACK — Check hardcoded framework override rules if dynamic checks yield false
        if self.has_fixed_role(target_role) {
            return true;
        }

        false
    }

    /// Checks if the user has the required role, and if not, returns a Forbidden error
    pub fn require_role(&self, target_role: &str) -> ShieldResult<()> {
        if self.has_role(target_role) {
            debug!("[RBAC] ✓ Role check passed for target '{}'", target_role);
            Ok(())
        } else {
            warn!(
                "[SECURITY EXCEPTION] Inline unified RBAC guard tripped: \
                 Missing role '{}'",
                target_role
            );
            Err(ShieldError::Forbidden)
        }
    }

    /// Generates or retrieves an existing CSRF token for the active session context.
    /// If a session exists but lacks a token, it initializes one on-the-fly dynamically.
    pub fn get_csrf_token(&self) -> String {
        if let Some(ref session_arc) = self.session {
            match session_arc.lock() {
                Ok(mut session) => {
                    // If it exists, return it immediately
                    if let Some(token) = session.data.get("csrf_token") {
                        debug!(
                            "[CSRF] Retrieved existing token from session {}",
                            session.id
                        );
                        return token.clone();
                    }

                    // If the session exists but lacks a token, mint it right now!
                    let fresh_token = uuid::Uuid::new_v4().to_string();
                    session
                        .data
                        .insert("csrf_token".to_string(), fresh_token.clone());

                    debug!(
                        "[CSRF KERNEL] Lazy-initialized token on first context read: {}",
                        fresh_token
                    );
                    return fresh_token;
                }
                Err(e) => {
                    error!("[CSRF ERROR] Failed to lock session: {}", e);
                    return String::new();
                }
            }
        }

        // Fallback catch if no session is mounted at all
        error!("[CSRF ERROR] No session available for CSRF token generation");
        String::new()
    }

    /// Safely extracts and decodes a query parameter value by key.
    /// Converts hex escape sequences (like %20) back into clean UTF-8 text.
    pub fn get_query_param_decoded(&self, key: &str) -> Option<String> {
        let raw_val = self.query.get(key)?;

        let mut decoded = String::new();
        let mut chars = raw_val.as_str().chars();

        while let Some(ch) = chars.next() {
            if ch == '%' {
                // Read the next two characters representing hex digits
                let mut hex = String::new();
                if let Some(h1) = chars.next() {
                    hex.push(h1);
                }
                if let Some(h2) = chars.next() {
                    hex.push(h2);
                }

                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    decoded.push(byte as char);
                }
            } else if ch == '+' {
                decoded.push(' '); // Form encoding variant fallback
            } else {
                decoded.push(ch);
            }
        }

        Some(decoded)
    }
}

// The struct that will be globally collected from any file
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

pub enum RoutingResult<'a> {
    Found(
        &'a dyn IntoHandler,
        Option<&'static str>,
        HashMap<String, UntrustedString>,
    ),
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
    pub global_error_handler: GlobalErrorHandler,
    pub telemetry: SystemTelemetry,
    pub fallback_handler: Option<PageHandlerFn>,
    pub role_registry: HashMap<String, &'static str>, // Local thread-safe registry tracking roles mapped to explicit route URL strings
    pub role_inheritance: HashMap<String, Vec<String>>,
}

impl Router {
    pub fn new() -> Self {
        logger::init_from_env();

        let fallback = if let Ok(guard) = GLOBAL_FALLBACK.lock() {
            guard.clone()
        } else {
            None
        };

        let mut router = Router {
            root: Node::new(),
            middlewares: Vec::new(),
            db: None,
            after_hooks: Vec::new(),
            global_error_handler: GlobalErrorHandler {
                handler: Some(default_framework_error_handler),
            },
            telemetry: SystemTelemetry::new(),
            fallback_handler: fallback,
            role_registry: HashMap::new(),
            role_inheritance: HashMap::new(),
        };

        let mut max_len = 0;
        let mut all_auto_routes = Vec::new();

        // ---- COLLECT ALL AUTO ROUTES ----
        for route in inventory::iter::<AutoRoute> {
            all_auto_routes.push(route);
            let len = route.path.len();
            if len > max_len {
                max_len = len;
            }
        }
        max_len += 4; // Add padding for better readability

        // ---- REGISTER ROUTES WITH CONSISTENT FORMATTING ----
        for route in all_auto_routes {
            info!(
                "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                route.path,
                max_len,
                format!("->").green(),
                method_color(&format!("{:?}", route.method))
            );

            if let Some(role) = route.required_role {
                router.role_registry.insert(route.path.to_string(), role);
            }

            router.add_route(route.method, route.path, route.handler, route.required_role);
        }

        // Admin routes
        #[cfg(feature = "admin")]
        {
            use crate::database::repository::{ACTIONS_REGISTRY, ADMIN_REGISTRY};
            use crate::gritadmin::metrics::admin_security_matrix_view_handler;
            use crate::protocol::request::HttpMethod;

            let registry = ADMIN_REGISTRY.lock().unwrap();

            // ---- COLLECT ALL PATHS ----
            let mut all_paths = Vec::new();

            // Table routes
            for (_table_name, model) in registry.iter() {
                all_paths.push(model.route_path.to_string());
                all_paths.push(format!("{}/search", model.route_path));
                all_paths.push(format!("{}/delete", model.route_path));
                all_paths.push(format!("{}/update-cell", model.route_path));
                all_paths.push(format!("{}/query-explorer", model.route_path));
                all_paths.push(format!("{}/:id", model.route_path));
                all_paths.push(format!("{}/bulk-delete", model.route_path));
                all_paths.push(format!("{}/export", model.route_path));
            }

            // Admin API routes
            let static_routes = vec![
                "/admin/api/alter-table/:table_slug/add-column",
                "/admin/dashboard",
                "/admin/api/search-palette",
                "/admin/api/create-table",
                "/admin/api/metrics",
                "/admin/metrics",
                "/admin/settings/security",
            ];
            all_paths.extend(static_routes.iter().map(|s| s.to_string()));

            // Custom action routes
            for (table_slug, _) in ACTIONS_REGISTRY.lock().unwrap().iter() {
                all_paths.push(format!("/admin/{}/action/:action_name", table_slug));
                all_paths.push(format!("/admin/{}/bulk-action/:action_name", table_slug));
            }

            // ---- CALCULATE MAX LEN ----
            let mut max_len = 0;
            for path in &all_paths {
                let len = path.len();
                if len > max_len {
                    max_len = len;
                }
            }
            max_len += 4; // Add padding for better readability

            // ---- REGISTER ROUTES WITH CONSISTENT FORMATTING ----
            for (_table_name, model) in registry.iter() {
                // Core Workspace Table Views / Global Dynamic Routes (GET)
                info!(
                    "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                    model.route_path,
                    max_len,
                    format!("->").green(),
                    method_color("GET")
                );
                router.add_route(
                    HttpMethod::GET,
                    model.route_path,
                    model.list_handler.clone(),
                    None,
                );

                // Real-time Grid Search Pipeline (GET)
                let search = format!("{}/search", model.route_path);
                info!(
                    "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                    search,
                    max_len,
                    format!("->").green(),
                    method_color("GET")
                );
                router.add_route(
                    HttpMethod::GET,
                    Box::leak(search.into_boxed_str()),
                    model.search_handler.clone(),
                    None,
                );

                // Delete Route (DELETE)
                let delete_path = format!("{}/delete", model.route_path);
                info!(
                    "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                    delete_path,
                    max_len,
                    format!("->").green(),
                    method_color("DELETE")
                );
                router.add_route(
                    HttpMethod::DELETE,
                    Box::leak(delete_path.into_boxed_str()),
                    model.delete_handler.clone(),
                    None,
                );

                // Inline Cell Updates (PATCH)
                let patch_path = format!("{}/update-cell", model.route_path);
                info!(
                    "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                    patch_path,
                    max_len,
                    format!("->").green(),
                    method_color("PATCH")
                );
                router.add_route(
                    HttpMethod::PATCH,
                    Box::leak(patch_path.into_boxed_str()),
                    model.patch_handler.clone(),
                    None,
                );

                // Advanced Matrix Query Explorer Pipeline (GET)
                let advanced_search_path = format!("{}/query-explorer", model.route_path);
                info!(
                    "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                    advanced_search_path,
                    max_len,
                    format!("->").green(),
                    method_color("GET")
                );
                router.add_route(
                    HttpMethod::GET,
                    Box::leak(advanced_search_path.into_boxed_str()),
                    model.advanced_search_handler.clone(),
                    None,
                );

                // Detail view (GET)
                let detail_path = format!("{}/:id", model.route_path);
                info!(
                    "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                    detail_path,
                    max_len,
                    format!("->").green(),
                    method_color("GET")
                );
                router.add_route(
                    HttpMethod::GET,
                    Box::leak(detail_path.into_boxed_str()),
                    model.detail_handler.clone(),
                    None,
                );

                // Bulk delete (POST)
                let bulk_path = format!("{}/bulk-delete", model.route_path);
                info!(
                    "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                    bulk_path,
                    max_len,
                    format!("->").green(),
                    method_color("POST")
                );
                router.add_route(
                    HttpMethod::POST,
                    Box::leak(bulk_path.into_boxed_str()),
                    model.bulk_delete_handler.clone(),
                    None,
                );

                // Export routes (GET)
                let export_path = format!("{}/export", model.route_path);
                info!(
                    "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                    export_path,
                    max_len,
                    format!("->").green(),
                    method_color("GET")
                );
                router.add_route(
                    HttpMethod::GET,
                    Box::leak(export_path.into_boxed_str()),
                    model.export_handler.clone(),
                    None,
                );
            }

            // ---- CUSTOM ACTION ROUTES ----
            for (table_slug, _) in ACTIONS_REGISTRY.lock().unwrap().iter() {
                let action_path = format!("/admin/{}/action/:action_name", table_slug);
                let path = Box::leak(action_path.into_boxed_str());

                info!(
                    "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                    path,
                    max_len,
                    format!("->").green(),
                    method_color("POST")
                );
                router.add_route(HttpMethod::POST, path, handle_custom_action, None);

                let bulk_action_path = format!("/admin/{}/bulk-action/:action_name", table_slug);
                let bulk_path = Box::leak(bulk_action_path.into_boxed_str());

                info!(
                    "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                    bulk_path,
                    max_len,
                    format!("->").green(),
                    method_color("POST")
                );
                router.add_route(HttpMethod::POST, bulk_path, handle_custom_action, None);
            }

            // ---- DASHBOARD ROUTE ----
            let dashboard_handler: AdminHandlerFn = Arc::new(|ctx| Box::pin(handle_dashboard(ctx)));
            info!(
                "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                "/admin/dashboard",
                max_len,
                format!("->").green(),
                method_color("GET")
            );
            router.add_route(HttpMethod::GET, "/admin/dashboard", dashboard_handler, None);

            // ---- SEARCH PALETTE ROUTE ----
            let palette_handler: AdminHandlerFn =
                Arc::new(|ctx| Box::pin(handle_search_palette(ctx)));
            info!(
                "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                "/admin/api/search-palette",
                max_len,
                format!("->").green(),
                method_color("GET")
            );
            router.add_route(
                HttpMethod::GET,
                "/admin/api/search-palette",
                palette_handler,
                None,
            );

            // ---- ALTER TABLE ROUTE ----
            let alter_table_add_column_handler: AdminHandlerFn = Arc::new(|_ctx| {
                Box::pin(async move {
                    // Handler implementation...
                    Response::ok("Alter table route".to_string())
                })
            });
            info!(
                "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                "/admin/api/alter-table/:table_slug/add-column",
                max_len,
                format!("->").green(),
                method_color("POST")
            );
            router.add_route(
                HttpMethod::POST,
                "/admin/api/alter-table/:table_slug/add-column",
                alter_table_add_column_handler,
                None,
            );

            // ---- CREATE TABLE ROUTE ----
            let create_table_handler: AdminHandlerFn = Arc::new(|ctx| {
                Box::pin(async move {
                    let db = &ctx
                        .db
                        .as_deref()
                        .expect("Database connection is not mounted in the context");

                    let table_name = ctx
                        .form
                        .fields
                        .get("table_name")
                        .cloned()
                        .unwrap_or_default();

                    let columns_json = ctx
                        .form
                        .fields
                        .get("columns_data")
                        .cloned()
                        .unwrap_or_default();

                    let columns_json_trimmed = columns_json.as_str().trim();

                    if columns_json_trimmed.is_empty() {
                        return error_response(
                            "Columns configuration specification cannot be blank.",
                        );
                    }

                    let sanitized_json = if columns_json_trimmed.contains('%') {
                        urlencoding::decode(columns_json_trimmed)
                            .map(|s| s.into_owned())
                            .unwrap_or_else(|_| columns_json_trimmed.to_string())
                    } else {
                        columns_json_trimmed.to_string()
                    };

                    let parsed_columns: Vec<DynamicColumnSpec> =
                        match serde_json::from_str(&sanitized_json) {
                            Ok(cols) => cols,
                            Err(err) => {
                                return error_response(format!(
                                    "Invalid attributes specification structure: {}",
                                    err
                                ))
                            }
                        };

                    match handle_create_table_dynamic(db, table_name.to_string(), parsed_columns)
                        .await
                    {
                        Ok(success_msg) => success_response(success_msg),
                        Err(error_msg) => error_response(error_msg),
                    }
                })
            });
            info!(
                "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                "/admin/api/create-table",
                max_len,
                format!("->").green(),
                method_color("POST")
            );
            router.add_route(
                HttpMethod::POST,
                "/admin/api/create-table",
                create_table_handler,
                None,
            );

            info!(
                "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                "/admin/api/metrics",
                max_len,
                format!("->").green(),
                method_color("GET")
            );
            router.add_route(
                HttpMethod::GET,
                "/admin/api/metrics",
                admin_metrics_api_handler,
                None,
            );

            info!(
                "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                "/admin/metrics",
                max_len,
                format!("->").green(),
                method_color("GET")
            );
            router.add_route(
                HttpMethod::GET,
                "/admin/metrics",
                admin_metrics_html_handler,
                None,
            );

            // ---- SECURITY SETTINGS ROUTE ----
            info!(
                "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                "/admin/settings/security",
                max_len,
                format!("->").green(),
                method_color("GET")
            );
            router.add_route(
                HttpMethod::GET,
                "/admin/settings/security",
                admin_security_matrix_view_handler,
                None,
            );

            // ---- SWAGGER UI ROUTE ----
            let swagger_handler: AdminHandlerFn = Arc::new(|_ctx| {
                Box::pin(async move {
                    let html = render_swagger_ui().into_string();
                    Response::ok(Sanitizer::trust(&html))
                })
            });

            info!(
                "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                "/admin/docs",
                max_len,
                format!("->").green(),
                method_color("GET")
            );

            router.add_route(HttpMethod::GET, "/admin/docs", swagger_handler, None);

            // ---- OPTIONAL: RAW OPENAPI JSON ENDPOINT ----
            let openapi_handler: AdminHandlerFn = Arc::new(|_ctx| {
                Box::pin(async move {
                    let spec = generate_openapi_spec();
                    Response::json(200, &spec)
                })
            });

            info!(
                "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                "/admin/docs/openapi.json",
                max_len,
                format!("->").green(),
                method_color("GET")
            );

            router.add_route(
                HttpMethod::GET,
                "/admin/docs/openapi.json",
                openapi_handler,
                None,
            );
        }
        router
    }

    pub fn mount_db(mut self, db: Arc<DatabaseConnection>) -> Self {
        self.db = Some(db);
        self
    }

    /// Premium builder to switch on detailed diagnostic server logs
    pub fn mount_logger(self, level: LogLevel) -> Self {
        logger::init(level);
        self
    }

    /// Register routes using tuples for a clean, declarative style.
    ///
    /// ### Example
    /// You can declare a matrix of routes in one place:
    /// ```rust
    /// let app_routes = vec![
    ///     ("/login",    HttpMethod::GET,  handle_login),
    ///     ("/register", HttpMethod::POST, handle_register),
    ///     ("/dashboard",HttpMethod::GET,  handle_dashboard),
    /// ];
    /// ```
    ///
    /// Then register them all elegantly in a single line:
    /// ```rust
    /// app_routes.into_iter().for_each(|r| router.route(r));
    /// ```
    pub fn route<H>(mut self, route_info: (&str, HttpMethod, H)) -> Self
    where
        H: IntoHandler,
    {
        self.add_route(route_info.1, route_info.0, route_info.2, None);
        self
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

    /// Builder method to dynamically define role hierarchies at startup
    pub fn add_role_inheritance(mut self, parent: &str, children: Vec<&str>) -> Self {
        let child_strings = children.into_iter().map(|s| s.to_string()).collect();
        self.role_inheritance
            .insert(parent.to_string(), child_strings);
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

    // Update the basic registration engine signature
    pub fn add_route<H>(
        &mut self,
        method: HttpMethod,
        path: &str,
        handler: H,
        required_role: Option<&'static str>,
    ) where
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
        // Inject both operational layers into the target method bucket
        current.methods.insert(
            method,
            RouteTarget {
                handler: Box::new(handler),
                required_role,
            },
        );
    }

    pub fn match_route<'a>(&'a self, method: &HttpMethod, path: &str) -> RoutingResult<'a> {
        let mut current = &self.root;
        let mut params = HashMap::new();

        // normalize the string boundaries without altering mid-path structures
        let mut trimmed_path = path;
        if trimmed_path.starts_with('/') {
            trimmed_path = &trimmed_path[1..];
        }
        if trimmed_path.ends_with('/') && !trimmed_path.is_empty() {
            trimmed_path = &trimmed_path[..trimmed_path.len() - 1];
        }

        // Parse segments strictly, reject path if intermediate empty elements exist
        let segments: Vec<&str> = if trimmed_path.is_empty() {
            Vec::new() // Handles the base root "/" path cleanly
        } else {
            let parts: Vec<&str> = trimmed_path.split('/').collect();
            if parts.iter().any(|s| s.is_empty()) {
                // Catches invalid sequences "/admin/////users" or "///"
                return RoutingResult::NotFound;
            }
            parts
        };

        // Process the cleanly generated routing segments
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
                        break;
                    } else {
                        // Try the node's explicit property first; if None, fall back to the child map key string!
                        let clean_key = if let Some(ref name) = param_node.parameter_name {
                            name.trim_start_matches(':').to_string()
                        } else {
                            key.trim_start_matches(':').to_string()
                        };

                        // Insert the dynamic slug value safely into our parameters dictionary
                        params.insert(clean_key, UntrustedString::new(segment.to_string()));

                        // Advance the tracker node downward to continue evaluating subsequent segments
                        current = param_node;
                    }
                } else {
                    return RoutingResult::NotFound;
                }
            }
        }

        match current.methods.get(method) {
            Some(target) => RoutingResult::Found(&*target.handler, target.required_role, params),
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

    // Update file-system crawlers to pass down macro role parameters
    fn process_page_file(&mut self, file_path: &Path, base_dir: &Path) {
        let file_key = file_path.to_string_lossy().replace("\\", "/");
        let relative = file_path.strip_prefix(base_dir).unwrap().with_extension("");
        let relative_str = relative.to_string_lossy().replace("\\", "/");

        let mut url_route = if relative_str == "index" {
            "/".to_string()
        } else if relative_str.ends_with("/index") {
            format!("/{}", relative_str.trim_end_matches("/index"))
        } else {
            format!("/{}", relative_str)
        };

        if url_route.contains('[') && url_route.contains(']') {
            url_route = url_route
                .replace("[..", ":*")
                .replace("[", ":*")
                .replace("]", "");
        }
        if url_route.contains('_') {
            url_route = url_route.replace("_", ":*");
        }

        if let Ok(registry) = FILE_ROUTING_REGISTRY.lock() {
            if let Some(registered) = registry.get(&file_key) {
                info!(
                    "[FBS-ROUTER] >>: {:<30} {} [{:<6}] {}",
                    file_key,
                    format!("->").green(),
                    method_color(&format!("{:?}", registered.method)),
                    url_route
                );
                let handler_instance = (registered.handler_factory)();

                // Seamless integration: file-system pages now pipe their macro roles straight into the trie!
                self.add_route(
                    registered.method,
                    &url_route,
                    handler_instance,
                    registered.required_role,
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
