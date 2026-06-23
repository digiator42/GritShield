use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::core::env::get_env;
use crate::protocol::request::HttpMethod;
use crate::protocol::response::Response;
use crate::protocol::response::{Cookie, SameSite};
use crate::routing::trie::RequestContext;
use crate::security::jwt::{Claims, JwtHandler};
use crate::security::rate_limit::RateLimiter;
use crate::security::session::{Session, SessionStore};
use crate::security::xss::Sanitizer;
use crate::{debug, error};
use chrono::Local;
use colored::*;
use uuid::Uuid;

pub enum MiddlewareResult {
    Next(Option<MiddlewareState>), // State can hold session data, claims, or both
    Error(Response),               // Stop and return error immediately
}

// A state packer to carry data down the pipe safely
pub struct MiddlewareState {
    pub session: Option<Arc<Mutex<Session>>>,
    pub claims: Option<Claims>,
    pub session_was_stale: bool,
}

pub trait Middleware: Send + Sync {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult;
}

pub trait AfterRequestHook: Send + Sync {
    fn call(&self, ctx: &RequestContext, status: u16, duration: std::time::Duration);
}

pub struct AuthMiddleware {
    pub store: Arc<SessionStore>,
    pub jwt_handler: Option<JwtHandler>,
    pub public_paths: Vec<String>, // List of open routes
    pub enable_csrf: bool,
    pub redirect: Option<String>,
}

impl AuthMiddleware {
    /// Pure Stateful Session Architecture (No JWTs)
    pub fn new_session(public_paths: Vec<String>, redirect: Option<&str>) -> Self {
        let session_store = Arc::new(SessionStore::new());

        Self {
            store: session_store,
            jwt_handler: None,
            public_paths,
            enable_csrf: true,
            redirect: redirect.map(|s| s.to_string()).or(None),
        }
    }

    /// Pure Stateless JWT Architecture (Dummy empty session store bypassed)
    pub fn new_jwt(
        jwt_handler: JwtHandler,
        public_paths: Vec<String>,
        redirect: Option<&str>,
    ) -> Self {
        Self {
            store: Arc::new(SessionStore::new()),
            jwt_handler: Some(jwt_handler),
            public_paths,
            enable_csrf: false,
            redirect: redirect.map(|s| s.to_string()).or(None),
        }
    }

    /// Internal helper to evaluate whether a path matches a whitelisted path rule
    fn is_public(&self, incoming_path: &str) -> bool {
        let incoming_segments: Vec<&str> =
            incoming_path.split('/').filter(|s| !s.is_empty()).collect();

        for rule in &self.public_paths {
            let rule_segments: Vec<&str> = rule.split('/').filter(|s| !s.is_empty()).collect();

            // Handle explicit root match ("/")
            if rule == "/" && incoming_path == "/" {
                return true;
            }

            // Check for catch-all "**" FIRST
            if rule_segments.len() >= 1 && (rule_segments[rule_segments.len() - 1] == "**") {
                let rule_prefix_len = rule_segments.len() - 1;

                if incoming_segments.len() >= rule_prefix_len {
                    let mut all_match = true;
                    for i in 0..rule_prefix_len {
                        if rule_segments[i] != incoming_segments[i] {
                            all_match = false;
                            break;
                        }
                    }
                    if all_match {
                        return true;
                    }
                }
                // If prefix doesn't match, skip to next rule
                continue;
            }

            // Check for ":*" wildcard catch-all
            if rule_segments.len() >= 1 && rule_segments[rule_segments.len() - 1].starts_with(":*")
            {
                let rule_prefix_len = rule_segments.len() - 1;

                if incoming_segments.len() >= rule_prefix_len {
                    let mut all_match = true;
                    for i in 0..rule_prefix_len {
                        if rule_segments[i] != incoming_segments[i] {
                            all_match = false;
                            break;
                        }
                    }
                    if all_match {
                        return true;
                    }
                }
                // If prefix doesn't match, skip to next rule
                continue;
            }

            // Exact path matching (no wildcards)
            let mut matches = true;
            for (i, rule_seg) in rule_segments.iter().enumerate() {
                if i >= incoming_segments.len() {
                    matches = false;
                    break;
                }

                if *rule_seg == "*" {
                    continue;
                }

                if rule_seg != &incoming_segments[i] {
                    matches = false;
                    break;
                }
            }

            if matches && incoming_segments.len() == rule_segments.len() {
                return true;
            }
        }

        false
    }
}

impl Middleware for AuthMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // -----------------------------------------------------------------
        // STEP 1: ENHANCED PUBLIC ROUTE BYPASS & LOOP PREVENTION
        // -----------------------------------------------------------------
        let is_public_route = self.is_public(&ctx.req.path);

        debug!(
            "[AUTH MIDDLEWARE] Evaluating route: {} | Public: {}",
            ctx.req.path, is_public_route
        );

        // If it's a public route (like /auth/login, /auth/register, or static assets),
        // bypass CSRF and strict auth redirect gates completely!
        if is_public_route {
            let mut associated_session = None;

            // Try to find an existing session from the browser's cookie jar
            if let Some(sid) = ctx.get_signed_cookie("GSESSION_ID") {
                if let Ok(store_guard) = self.store.sessions.lock() {
                    if let Some(session_ptr) = store_guard.get(&sid) {
                        debug!("[AUTH MIDDLEWARE] ✓ Found session in store: {}", sid);
                        associated_session = Some(Arc::clone(&session_ptr));
                    } else {
                        debug!(
                            "[AUTH MIDDLEWARE] ✗ Session ID in cookie but NOT in store: {}",
                            sid
                        );
                    }
                }
            } else {
                debug!("[AUTH MIDDLEWARE] No GSESSION_ID cookie found");
            }

            // If no session exists, mint one on-the-fly right here
            if associated_session.is_none() && self.jwt_handler.is_none() {
                debug!("[AUTH MIDDLEWARE] Creating NEW session (no existing session found)");
                if let Ok(store_guard) = self.store.sessions.lock() {
                    let new_sid = uuid::Uuid::new_v4().to_string();
                    debug!("[AUTH MIDDLEWARE] Generated session ID: {}", new_sid);
                    let new_session = Arc::new(Mutex::new(Session {
                        id: new_sid.clone(),
                        data: std::collections::HashMap::new(),
                        user_id: None,
                        last_accessed: std::time::Instant::now(),
                    }));

                    store_guard.insert(new_sid.clone(), Arc::clone(&new_session));

                    let is_production = get_env("APP_ENV", "development") == "production";
                    let session_cookie = Cookie::new("GSESSION_ID", &new_sid)
                        .set_secure(is_production)
                        .set_same_site(SameSite::Lax);

                    ctx.set_signed_cookie(session_cookie);
                    debug!(
                        "[AUTH MIDDLEWARE] ✓ Set GSESSION_ID cookie & into store: {}",
                        new_sid
                    );
                    associated_session = Some(new_session);
                }
            } else if associated_session.is_some() {
                debug!("[AUTH MIDDLEWARE] ✓ Using existing session");
            }

            // Sync context request state seamlessly
            ctx.session = associated_session.clone();

            return MiddlewareResult::Next(Some(MiddlewareState {
                session: associated_session,
                claims: None,
                session_was_stale: false,
            }));
        }

        // -----------------------------------------------------------------
        // STEP 2: LOGOUT INTERCEPTION
        // -----------------------------------------------------------------
        if ctx.req.path == "/logout" {
            debug!("[AUTH KERNEL] Intercepted explicit /logout pathway trigger.");

            if let Some(session_id) = ctx.get_signed_cookie("GSESSION_ID") {
                if let Ok(store_guard) = self.store.sessions.lock() {
                    store_guard.remove(&session_id);
                    debug!(
                        "[AUTH KERNEL] Successfully removed session ID {} from memory pool.",
                        session_id
                    );
                }
            }

            let mut delete_cookie = Cookie::new("GSESSION_ID", "");
            delete_cookie.max_age = 0;

            let is_production = get_env("APP_ENV", "development") == "production";
            let delete_cookie = delete_cookie
                .set_secure(is_production)
                .set_same_site(SameSite::Lax);

            ctx.set_signed_cookie(delete_cookie);

            if let Some(ref redirect_path) = self.redirect {
                return MiddlewareResult::Error(Response::redirect(303, redirect_path));
            }
            return MiddlewareResult::Error(Response::redirect(303, "/"));
        }

        // -----------------------------------------------------------------
        // STEP 3: PRIVATE ROUTE LIFECYCLE (Sessions & CSRF Guard)
        // -----------------------------------------------------------------
        let mut active_session = None;
        let running_sessions =
            self.jwt_handler.is_none() || ctx.get_signed_cookie("GSESSION_ID").is_some();
        let mut cookie_was_stale = false;

        if running_sessions {
            let session_id: Option<String> = ctx.get_signed_cookie("GSESSION_ID");

            let store_guard = match self.store.sessions.lock() {
                Ok(guard) => guard,
                Err(_) => {
                    return MiddlewareResult::Error(Response::new(
                        500,
                        Sanitizer::trust(
                            "<h1>500 Internal Server Error: Session Pool Poisoned</h1>",
                        ),
                    ));
                }
            };

            if let Some(ref sid) = session_id {
                if let Some(session_ptr) = store_guard.get(sid) {
                    ctx.session = Some(Arc::clone(&session_ptr));
                    active_session = Some(Arc::clone(&session_ptr));
                } else {
                    cookie_was_stale = true;
                }
            }

            if active_session.is_none() && self.jwt_handler.is_none() {
                let new_sid = uuid::Uuid::new_v4().to_string();
                let new_session = Arc::new(Mutex::new(Session {
                    id: new_sid.clone(),
                    data: std::collections::HashMap::new(),
                    user_id: None,
                    last_accessed: std::time::Instant::now(),
                }));

                store_guard.insert(new_sid.clone(), Arc::clone(&new_session));

                let is_production = get_env("APP_ENV", "development") == "production";
                let session_cookie = Cookie::new("GSESSION_ID", &new_sid)
                    .set_secure(is_production)
                    .set_same_site(SameSite::Lax);

                ctx.set_signed_cookie(session_cookie);
                ctx.session = Some(Arc::clone(&new_session));
                active_session = Some(new_session);
            }

            if cookie_was_stale {
                let mut delete_cookie = Cookie::new("GSESSION_ID", "");
                delete_cookie.max_age = 0;
                ctx.set_signed_cookie(delete_cookie);
            }

            drop(store_guard); // Release lock cleanly

            if let Some(ref session_arc) = active_session {
                let mut session = session_arc.lock().unwrap();
                if !session.data.contains_key("csrf_token") {
                    let fresh_token = uuid::Uuid::new_v4().to_string();
                    session.data.insert("csrf_token".to_string(), fresh_token);
                    debug!(
                    "[CSRF KERNEL] Initialized unique persistent anti-forgery token for session context."
                );
                }
            }
        }

        // -----------------------------------------------------------------
        // STEP 4: CSRF GUARD FOR STATE-CHANGING METHODS
        // -----------------------------------------------------------------
        if self.enable_csrf && self.jwt_handler.is_none() {
            let method = ctx.req.method;
            if method == HttpMethod::POST
                || method == HttpMethod::PUT
                || method == HttpMethod::PATCH
                || method == HttpMethod::DELETE
            {
                let mut csrf_verified = false;
                let mut incoming_token: Option<String> =
                    ctx.headers.get("x-csrf-token").map(|s| s.to_string());

                if incoming_token.is_none() {
                    let form_data = ctx.req.parse_form_body();
                    if let Some(form_val) = form_data.fields.get("csrf_token") {
                        incoming_token = Some(form_val.to_string());
                    }
                }

                if let Some(ref session_arc) = active_session {
                    let session = session_arc.lock().unwrap();
                    if let Some(session_token) = session.data.get("csrf_token") {
                        if let Some(ref untrusted) = incoming_token {
                            debug!(
                            "[CSRF GUARD] Comparing memory token [{}] against incoming challenge token [{}]",
                            session_token, untrusted
                        );
                            if session_token == untrusted {
                                csrf_verified = true;
                            }
                        }
                    }
                }

                if !csrf_verified {
                    debug!(
                        "\x1b[31m[SECURITY ALERT] CSRF Validation Failed for Route: {}\x1b[0m",
                        ctx.req.path
                    );
                    return MiddlewareResult::Error(Response::forbidden(
                        &std::collections::HashMap::from([(
                            "error",
                            "GritShield: Anti-Forgery Token Validation Rejected.",
                        )]),
                    ));
                }
            }
        }

        // -----------------------------------------------------------------
        // STEP 5: EVALUATE AUTHENTICATION STATUS (Session vs JWT)
        // -----------------------------------------------------------------
        if let Some(ref session_arc) = active_session {
            let session = session_arc.lock().unwrap();
            if session.data.get("user_id").is_some() {
                drop(session);
                return MiddlewareResult::Next(Some(MiddlewareState {
                    session: active_session.clone(),
                    claims: None,
                    session_was_stale: false,
                }));
            }
        }

        // Fallback check against incoming JWT Bearer tokens
        if let Some(jwt_handler) = &self.jwt_handler {
            if let Some(auth_header) = ctx.headers.get("authorization") {
                if auth_header.starts_with("Bearer ") {
                    let token = &auth_header[7..];

                    match jwt_handler.verify(token) {
                        Ok(claims) => {
                            debug!("[AUTH] Verified user token: {}", claims.sub);
                            ctx.claims = Some(claims.clone());

                            return MiddlewareResult::Next(Some(MiddlewareState {
                                session: active_session.clone(),
                                claims: Some(claims),
                                session_was_stale: false,
                            }));
                        }
                        Err(e) => {
                            debug!("[AUTH] Token validation rejected: {}", e);
                        }
                    }
                }
            }
        }

        // -----------------------------------------------------------------
        // STEP 6: FALLBACK ENFORCEMENT REDIRECT OR 401 UNAUTHORIZED
        // -----------------------------------------------------------------
        if let Some(ref redirect_path) = self.redirect {
            debug!(
                "\x1b[33m[AUTH] Unauthenticated attempt to private route {}. Redirecting...\x1b[0m",
                ctx.req.path
            );
            return MiddlewareResult::Error(Response::redirect(303, redirect_path));
        }

        let err_body = Sanitizer::trust("<h1>401 Unauthorized</h1><p>Access Denied.</p>");
        MiddlewareResult::Error(Response::new(401, err_body))
    }
}

pub struct RateLimitMiddleware {
    pub limiter: RateLimiter,
}

impl Middleware for RateLimitMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // SECURELY resolve the true user identity string
        let client_ip = ctx.resolve_client_ip();

        // logging to see who is making requests
        println!(
            "[GRITSHIELD RATE-LIMIT] Evaluating limits for bucket identifier: {}",
            client_ip
        );

        if self.limiter.is_allowed(client_ip) {
            // Pass execution forward down the routing chain
            MiddlewareResult::Next(None)
        } else {
            // Client hit the ceiling limit! Push back with an explicit HTTP 429 back-off directive
            let err_body = Sanitizer::trust(
                "<h1>429 Too Many Requests</h1>\
                 <p>Slow down, friend. Your API bucket limits have been exhausted.</p>",
            );

            let mut res = Response::new(429, err_body);

            // Instruct downstream agents/browsers exactly how long to wait before trying again
            res.headers
                .push(("Retry-After".to_string(), "60".to_string()));
            res.headers.push((
                "Content-Type".to_string(),
                "text/html; charset=utf-8".to_string(),
            ));

            MiddlewareResult::Error(res)
        }
    }
}

pub struct IPBlacklistMiddleware {
    // Using HashSet for high-performance O(1) lookups
    pub blacklisted_ips: HashSet<String>,
}

impl IPBlacklistMiddleware {
    /// Constructor helper to instantiate the blacklist layer smoothly
    pub fn new(ips: Vec<&str>) -> Self {
        let mut set = HashSet::new();
        for ip in ips {
            set.insert(ip.to_string());
        }
        Self {
            blacklisted_ips: set,
        }
    }
}

impl Middleware for IPBlacklistMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Leverage your secure IP resolver from earlier
        let client_ip = ctx.resolve_client_ip();

        // Check the HashSet instantly
        if self.blacklisted_ips.contains(&client_ip) {
            error!(
                "[SECURITY ALERT] Blocked request attempt from blacklisted IP: {}",
                client_ip
            );

            ctx.telemetry
                .total_blocked_ips
                .fetch_add(1, Ordering::SeqCst);

            let err_body = Sanitizer::trust(
                "<h1>403 Forbidden</h1>\
                 <p>Access denied. Your IP address has been blocked.</p>",
            );

            let mut res = Response::new(403, err_body);
            res.headers.push((
                "Content-Type".to_string(),
                "text/html; charset=utf-8".to_string(),
            ));

            // Drop connection right here! Do not proceed to handlers or next middlewares
            MiddlewareResult::Error(res)
        } else {
            // All clear. Move down the execution pipeline chain smoothly
            MiddlewareResult::Next(None)
        }
    }
}

pub struct MetricsTracker;

impl AfterRequestHook for MetricsTracker {
    fn call(&self, ctx: &RequestContext, status: u16, _: std::time::Duration) {
        if status >= 500 {
            error!(
                "🚨 [ALERT] Critical server failure detected on path: {}",
                ctx.req.path
            );
        }
    }
}

pub struct CorsMiddleware {
    allowed_origins: Vec<String>,
}

impl CorsMiddleware {
    /// Accept an array or vector of strings during initialization
    pub fn new(origins: Vec<String>) -> Self {
        Self {
            allowed_origins: origins,
        }
    }
}

impl Middleware for CorsMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Extract the origin the browser is currently calling from
        // (Adjust the header lookup syntax based on GritShield's exact req structure)
        let inbound_origin = ctx.req.headers.get("Origin").cloned().unwrap_or_default();

        // Determine the target match. If the domain is whitelisted, echo it!
        // Otherwise, fallback to your primary origin safely.
        let dynamic_origin = if self.allowed_origins.contains(&inbound_origin) {
            inbound_origin
        } else {
            self.allowed_origins
                .first()
                .cloned()
                .unwrap_or_else(|| "http://localhost:3000".to_string())
        };

        // Handle preflight checks
        if ctx.req.method == HttpMethod::OPTIONS || ctx.req.method == HttpMethod::UNKNOWN {
            let mut res = Response::ok("Preflight Allowed");

            res.headers.push((
                "Access-Control-Allow-Origin".to_string(),
                dynamic_origin.clone(),
            ));
            res.headers.push((
                "Access-Control-Allow-Methods".to_string(),
                "POST, GET, OPTIONS, PUT, PATCH, DELETE".to_string(),
            ));
            res.headers.push((
                "Access-Control-Allow-Headers".to_string(),
                "Content-Type, Authorization".to_string(),
            ));
            res.headers
                .push(("Access-Control-Max-Age".to_string(), "86400".to_string()));

            return MiddlewareResult::Error(res);
        }

        // Handle standard operations
        ctx.headers
            .insert("Access-Control-Allow-Origin".to_string(), dynamic_origin);
        ctx.headers.insert(
            "Access-Control-Allow-Methods".to_string(),
            "GET, POST, PUT, PATCH, DELETE, OPTIONS".to_string(),
        );
        ctx.headers.insert(
            "Access-Control-Allow-Headers".to_string(),
            "Content-Type, Authorization".to_string(),
        );

        MiddlewareResult::Next(None)
    }
}

pub struct LoggerMiddleware;

impl Middleware for LoggerMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        let now = Local::now();
        let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string();

        println!(
            "[{}] {} request to {}",
            timestamp.green(),
            format!("{:?}", ctx.req.method).blue(),
            ctx.req.path.yellow()
        );

        MiddlewareResult::Next(None)
    }
}
pub struct SessionMiddleware {
    pub store: Arc<SessionStore>,
}

impl Middleware for SessionMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // pull the cryptographically signed cookie (Tamper-proof!)
        let session_id = ctx.get_signed_cookie("session_id");

        let store = match self.store.sessions.lock() {
            Ok(guard) => guard,
            Err(_) => {
                return MiddlewareResult::Error(Response::new(
                    500,
                    Sanitizer::trust("<h1>500 Internal Server Error: Session Pool Poisoned</h1>"),
                ));
            }
        };

        // Try to look up an existing valid session in our memory map
        if let Some(ref sid) = session_id {
            if let Some(session_ptr) = store.get(sid) {
                // Attach the pointer copy directly to the active request context
                ctx.session = Some(Arc::clone(&session_ptr));
                return MiddlewareResult::Next(Some(MiddlewareState {
                    session: Some(Arc::clone(&session_ptr)),
                    claims: None,
                    session_was_stale: false,
                }));
            }
        }

        // No session found or it expired? Create one on the fly!
        let new_sid = Uuid::new_v4().to_string();
        let new_session = Arc::new(Mutex::new(Session {
            id: new_sid.clone(),
            data: std::collections::HashMap::new(),
            user_id: None,
            last_accessed: Instant::now(),
        }));

        // Insert into master framework memory tracking pool
        store.insert(new_sid.clone(), Arc::clone(&new_session));

        // Drop the secure signed cookie straight back into the browser's CookieJar
        let is_production = get_env("APP_ENV", "development") == "production";
        let session_cookie = Cookie::new("session_id", &new_sid)
            .set_secure(is_production) // Automatically true on prod, false on localhost HTTP
            .set_same_site(SameSite::Lax);

        if session_id == None {
            ctx.set_signed_cookie(session_cookie);
        }

        // Bind the freshly minted session into the current request flow
        ctx.session = Some(Arc::clone(&new_session));

        MiddlewareResult::Next(Some(MiddlewareState {
            session: Some(new_session),
            claims: None,
            session_was_stale: false,
        }))
    }
}
