use crate::core::env::get_env;
use crate::http::request::HttpMethod;
use crate::http::response::{Cookie, Response, SameSite};
use crate::middleware::{Middleware, MiddlewareResult, MiddlewareState};
use crate::routing::engine::RequestContext;
use crate::security::jwt::JwtHandler;
use crate::security::session::{Session, SessionStore};
use crate::security::xss::Sanitizer;
use crate::{debug, info};
use std::sync::{Arc, Mutex, OnceLock};

// global accessible, thread-safe cell for administrative/user auth session store
pub static SESSION_STORE: OnceLock<Arc<SessionStore>> = OnceLock::new();

/// Global helper to retrieve or safely initialize the shared admin session memory pool
pub fn get_session_store() -> &'static Arc<SessionStore> {
    SESSION_STORE.get_or_init(|| Arc::new(SessionStore::new()))
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
        let session_store = Arc::clone(get_session_store());

        Self {
            store: session_store,
            jwt_handler: None,
            public_paths,
            enable_csrf: false,
            redirect: redirect.map(|s| s.to_string()).or(None),
        }
    }

    /// Pure Stateless JWT Architecture (Dummy empty session store bypassed)
    pub fn new_jwt(
        jwt_handler: JwtHandler,
        public_paths: Vec<String>,
        redirect: Option<&str>,
    ) -> Self {
        let session_store = Arc::clone(get_session_store());

        Self {
            store: session_store,
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
                if let Some(session_ptr) = self.store.sessions.get(&sid) {
                    debug!("[AUTH MIDDLEWARE] ✓ Found session in store: {}", sid);
                    associated_session = Some(Arc::clone(&session_ptr));
                } else {
                    debug!(
                        "[AUTH MIDDLEWARE] ✗ Session ID in cookie but NOT in store: {}",
                        sid
                    );
                }
            } else {
                debug!("[AUTH MIDDLEWARE] No GSESSION_ID cookie found");
            }

            // If no session exists, mint one on-the-fly right here
            if associated_session.is_none() && self.jwt_handler.is_none() {
                debug!("[AUTH MIDDLEWARE] Creating NEW session (no existing session found)");
                let new_sid = uuid::Uuid::new_v4().to_string();
                debug!("[AUTH MIDDLEWARE] Generated session ID: {}", new_sid);
                let new_session = Arc::new(Mutex::new(Session {
                    id: new_sid.clone(),
                    data: std::collections::HashMap::new(),
                    user_id: None,
                    last_accessed: std::time::Instant::now(),
                }));

                self.store
                    .sessions
                    .insert(new_sid.clone(), Arc::clone(&new_session));

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
                self.store.sessions.remove(&session_id);
                debug!(
                    "[AUTH KERNEL] Successfully removed session ID {} from memory pool.",
                    session_id
                );
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

            if let Some(ref sid) = session_id {
                if let Some(session_ref) = self.store.sessions.get(sid) {
                    let session_ptr = session_ref.value().clone();
                    ctx.session = Some(Arc::clone(&session_ptr));
                    active_session = Some(session_ptr);
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

                self.store
                    .sessions
                    .insert(new_sid.clone(), Arc::clone(&new_session));

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
                    ctx.headers.get("x-csrf-token").and_then(|vals| vals.first().cloned());

                if incoming_token.is_none() {
                    let form_data = ctx.req.parse_form_body();
                    if let Some(form_val) = form_data.fields.get("csrf_token") {
                        if let Some(token) = form_val.first() {
                            incoming_token = Some(token.to_string());
                        }
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
                    info!(
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
            if let Some(auth_headers) = ctx.headers.get("authorization") {
                if let Some(auth_header) = auth_headers.first() {
                    if auth_header.starts_with("Bearer ") {
                        let token = &auth_header[7..];

                        match jwt_handler.verify(token) {
                            Ok(claims) => {
                                debug!("[AUTH MIDDLEWARE] Verified user token: {}", claims.sub);
                                ctx.claims = Some(claims.clone());

                                return MiddlewareResult::Next(Some(MiddlewareState {
                                    session: active_session.clone(),
                                    claims: Some(claims),
                                    session_was_stale: false,
                                }));
                            }
                            Err(e) => {
                                debug!("[AUTH MIDDLEWARE] Token validation rejected: {}", e);
                            }
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
                "\x1b[33m[AUTH MIDDLEWARE] Unauthenticated attempt to private route {}. Redirecting...\x1b[0m",
                ctx.req.path
            );
            return MiddlewareResult::Error(Response::redirect(303, redirect_path));
        }

        let err_body = Sanitizer::trust("<h1>401 Unauthorized</h1><p>Access Denied.</p>");
        MiddlewareResult::Error(Response::new(401, err_body))
    }
}
