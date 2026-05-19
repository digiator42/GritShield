use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::protocol::request::HttpMethod;
use crate::protocol::response::{Cookie, SameSite};
use crate::protocol::{request::Request, response::Response};
use crate::routing::trie::RequestContext;
use crate::security::jwt::{Claims, JwtHandler};
use crate::security::rate_limit::RateLimiter;
use crate::security::session::{Session, SessionStore};
use crate::security::xss::Sanitizer;
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
}

impl AuthMiddleware {
    /// Pure Stateful Session Architecture (No JWTs)
    pub fn new_session(public_paths: Vec<String>) -> Self {
        let session_store = Arc::new(SessionStore::new());

        Self {
            store: session_store,
            jwt_handler: None,
            public_paths,
            enable_csrf: true,
        }
    }

    /// Pure Stateless JWT Architecture (Dummy empty session store bypassed)
    pub fn new_jwt(jwt_handler: JwtHandler, public_paths: Vec<String>) -> Self {
        Self {
            store: Arc::new(SessionStore::new()),
            jwt_handler: Some(jwt_handler),
            public_paths,
            enable_csrf: false,
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

            let mut matches = true;
            for (i, rule_seg) in rule_segments.iter().enumerate() {
                // If we hit your wildcard token variant, everything past this point matches!
                if *rule_seg == "*" || rule_seg.starts_with(':') && rule_seg.contains('*') {
                    return true;
                }

                // If the incoming path is shorter than the rule segment check, it's not a match
                if i >= incoming_segments.len() {
                    matches = false;
                    break;
                }

                // Standard exact segment match evaluation
                if rule_seg != &incoming_segments[i] {
                    matches = false;
                    break;
                }
            }

            // If we checked all rule segments perfectly and length matches, it's a valid match
            if matches && incoming_segments.len() == rule_segments.len() {
                return true;
            }
        }

        false
    }
}

impl Middleware for AuthMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        let is_public_route = self.is_public(&ctx.req.path);
        let mut active_session = None;

        // --- STEP 1: CONDITIONAL SESSION LIEFOCYCLE ---
        // Only run session logic if we are NOT in exclusive JWT stateless mode
        let running_sessions =
            self.jwt_handler.is_none() || ctx.get_signed_cookie("GSESSION_ID").is_some();

        if running_sessions {
            let session_id = ctx.get_signed_cookie("GSESSION_ID");

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
                }
            }

            // Mint a session on-the-fly only if sessions are the primary auth method
            if active_session.is_none() && self.jwt_handler.is_none() {
                let new_sid = uuid::Uuid::new_v4().to_string();
                let new_session = Arc::new(Mutex::new(Session {
                    id: new_sid.clone(),
                    data: std::collections::HashMap::new(),
                    user_id: None,
                    last_accessed: std::time::Instant::now(),
                }));

                store_guard.insert(new_sid.clone(), Arc::clone(&new_session));

                let is_production =
                    crate::core::env::get_env("APP_ENV", "development") == "production";
                let session_cookie = Cookie::new("GSESSION_ID", &new_sid)
                    .set_secure(is_production)
                    .set_same_site(SameSite::Lax);

                if session_id.is_none() {
                    ctx.set_signed_cookie(session_cookie);
                }

                ctx.session = Some(Arc::clone(&new_session));
                active_session = Some(new_session);
            }

            drop(store_guard); // Release lock cleanly

            // REFRESH TOKEN ROTATION INJECTOR
            // Whenever a user requests or refreshes a page (GET), generate a new CSRF token immediately.
            if ctx.req.method == HttpMethod::GET {
                if let Some(ref session_arc) = active_session {
                    let mut session = session_arc.lock().unwrap();
                    let fresh_token = uuid::Uuid::new_v4().to_string();
                    session.data.insert("csrf_token".to_string(), fresh_token);
                }
            }
        }

        // --- STEP 2: BYPASS GATEKEEPING FOR PUBLIC ROUTES ---
        if is_public_route {
            return MiddlewareResult::Next(Some(MiddlewareState {
                session: active_session.clone(), // Use clone to preserve variable
                claims: None,
            }));
        }

        // --- STEP 2.5: CSRF STATEFUL PROTECTION GUARD ---
        if self.enable_csrf && self.jwt_handler.is_none() {
            let method = ctx.req.method; // HttpMethod enum

            // CSRF only attacks state-changing operations
            if method == HttpMethod::POST
                || method == HttpMethod::PUT
                || method == HttpMethod::PATCH
                || method == HttpMethod::DELETE
            {
                let mut csrf_verified = false;

                // 1. Extract form payload body
                let form_data = ctx.req.parse_form_body();

                // 2. Fetch token from user's authenticated session state
                if let Some(ref session_arc) = active_session {
                    let session = session_arc.lock().unwrap();

                    if let Some(session_token) = session.data.get("csrf_token") {
                        // Look for the incoming hidden token field inside URL-encoded or Multipart text fields
                        if let Some(incoming_untrusted) = form_data.fields.get("csrf_token") {
                            // Use a constant-time comparison helper if available, or a strict equality match
                            // Since it's an UntrustedString wrapper, unpack its inner reference safely
                            if session_token == incoming_untrusted.as_str() {
                                csrf_verified = true;
                            }
                        }
                    }
                }

                if !csrf_verified {
                    println!(
                        "\x1b[31m[SECURITY ALERT] CSRF Validation Failed for Route: {}\x1b[0m",
                        ctx.req.path
                    );
                    let err_body = Sanitizer::trust(
                        "<h1>403 Forbidden</h1><p>GritShield: CSRF Token Invalid or Missing.</p>",
                    );
                    return MiddlewareResult::Error(Response::new(403, err_body));
                }
            }
        }

        // --- STEP 3: PRIVATE ROUTE AUTHENTICATION EVALUATION ---

        // Strategy A: Session Check
        if let Some(ref session_arc) = active_session {
            let session = session_arc.lock().unwrap();
            if session.data.get("user_id").is_some() {
                drop(session);
                return MiddlewareResult::Next(Some(MiddlewareState {
                    session: active_session.clone(), // Cloned safely
                    claims: None,
                }));
            }
        }

        // Strategy B: Fallback check against incoming JWT Bearer tokens
        if let Some(jwt_handler) = &self.jwt_handler {
            if let Some(auth_header) = ctx.headers.get("authorization") {
                if auth_header.starts_with("Bearer ") {
                    let token = &auth_header[7..];

                    match jwt_handler.verify(token) {
                        Ok(claims) => {
                            println!("[AUTH] Verified user token: {}", claims.sub);
                            ctx.claims = Some(claims.clone());

                            return MiddlewareResult::Next(Some(MiddlewareState {
                                session: active_session.clone(), // Cloned safely
                                claims: Some(claims),
                            }));
                        }
                        Err(e) => {
                            println!("[AUTH] Token validation rejected: {}", e);
                        }
                    }
                }
            }
        }

        // --- STEP 4: AUTHENTICATION FAILURE ---
        let err_body =
            Sanitizer::trust("<h1>401 Unauthorized</h1><p>Access Denied by GritShield Core.</p>");
        MiddlewareResult::Error(Response::new(401, err_body))
    }
}

pub struct LoggerMiddleware;

impl Middleware for LoggerMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        println!(
            "[LOG] {} request to {}",
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
        let is_production = crate::core::env::get_env("APP_ENV", "development") == "production";
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
        }))
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
            eprintln!(
                "[SECURITY ALERT] Blocked request attempt from blacklisted IP: {}",
                client_ip
            );

            ctx.telemetry.total_blocked_ips.fetch_add(1, Ordering::SeqCst);

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
            eprintln!(
                "🚨 [ALERT] Critical server failure detected on path: {}",
                ctx.req.path
            );
        }
    }
}
