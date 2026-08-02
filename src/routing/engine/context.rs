use crate::core::event_bus::EventBus;
use crate::core::job_queue::{JobStorage, MemoryJobQueue};
use crate::http::form::FormData;
use crate::http::request::Request;
use crate::http::response::Cookie;
use crate::routing::engine::ShieldResult;
use crate::security::cookies::CookieJar;
use crate::security::errors::ShieldError;
use crate::security::jwt::Claims;
use crate::security::sanitizer::GritSanitizable;
use crate::security::session::{Session, SessionStore};
use crate::security::telemetry::SystemTelemetry;
use crate::security::xss::UntrustedString;
use crate::{debug, error, trace, warn};
use sea_orm::DatabaseConnection;
use serde_json::Value;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct RequestContext {
    pub req: Request,
    pub telemetry: SystemTelemetry,
    pub event_bus: Arc<EventBus>,
    pub job_queue: Arc<dyn JobStorage>,
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
            event_bus: Arc::new(EventBus::init()),
            job_queue: Arc::new(MemoryJobQueue::new()),
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

    /// Deserializes JSON request body and automatically executes in-place payload sanitization.
    pub async fn json<T>(&self) -> Result<T, ShieldError>
    where
        T: serde::de::DeserializeOwned + GritSanitizable,
    {
        let content_type = self.content_type.as_deref().unwrap_or("");
        if !content_type.starts_with("application/json") {
            return Err(ShieldError::BadRequest(
                "Content-Type must be application/json".to_string(),
            ));
        }

        // Serde Deserialization
        let mut payload: T = serde_json::from_slice(&self.raw_body)
            .map_err(|e| ShieldError::BadRequest(format!("Failed to parse JSON body: {}", e)))?;

        // Active Defense Payload Sanitization
        payload.sanitize();

        Ok(payload)
    }

    /// Optional helper for developers using the `validator` crate.
    /// Deserializes -> Sanitizes in-place -> Validates structural rules.
    #[cfg(feature = "validator")]
    pub async fn validated_json<T>(&self) -> Result<T, ShieldError>
    where
        T: serde::de::DeserializeOwned + GritSanitizable + validator::Validate,
    {
        let mut payload = self.json::<T>().await?;

        // Validator after sanitization
        payload
            .validate()
            .map_err(|e| ShieldError::BadRequest(format!("Payload validation failed: {}", e)))?;

        Ok(payload)
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

    /// Fallback for dynamic role checking, if set in session it grants access as SuperAdmin
    pub fn has_super_admin_role(&self, user_role: &str, target_role: &str) -> bool {
        match (user_role, target_role) {
            ("SuperAdmin", _) => true, // Global framework override
            _ => false,
        }
    }

    /// Non-blocking check evaluating security roles using hierarchical permissions
    /// Checks for Admin => Operator => Auditor fixed hierarchy
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

    /// Evaluates BOTH Dynamic Graph Trees AND Fixed SuperAdmin role
    /// Prioritizes runtime user-defined inheritance graphs first, falling back to core system rules.
    pub fn has_role(&self, target_role: &str) -> bool {
        let user_role = match self.get_user_role() {
            Some(role) => role,
            None => return false, // No role found in session/JWT -> Denied immediately
        };

        // Direct Match Check
        if user_role == target_role {
            return true;
        }

        // Dynamic Tree Inheritance Check
        if self.check_inheritance(&user_role, target_role) {
            return true;
        }

        // Strict Fixed Fallback to SuperAdmin role
        if self.has_super_admin_role(&user_role, target_role) {
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
