use crate::gritadmin::auth::creds::get_admin_credentials;
use crate::http::response::{Cookie, HttpStatus};
use crate::http::response::{JsonPayload, SameSite};
use crate::middleware::auth::get_session_store;
use crate::middleware::{Middleware, MiddlewareResult};
use crate::routing::engine::RequestContext;
use crate::security::session::{Session, SessionStore};
use crate::{debug, prelude::*, warn};
use serde_json::json;
use std::sync::{Arc, Mutex};

/// Renders the secure administrative login viewport
pub async fn render_login_page(ctx: RequestContext) -> Response {
    // If the administrator is already authenticated, bypass login and redirect straight to the dashboard
    if verify_and_get_admin_id(&ctx).is_some() {
        return Response::redirect(303, "/admin/dashboard");
    }

    // Render a pristine, framework-branded CSS login page using raw HTML
    let login_html = r#"
        <!DOCTYPE html>
        <html lang="en">
        <head>
            <meta charset="UTF-8">
            <meta name="viewport" content="width=device-width, initial-scale=1.0">
            <title>GritShield Administrative Core</title>
            <style>
                body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; background: #0f172a; color: #f8fafc; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; }
                .login-card { background: #1e293b; padding: 2.5rem; border-radius: 12px; box-shadow: 0 10px 25px -5px rgba(0,0,0,0.3); width: 100%; max-width: 400px; border: 1px solid #334155; }
                h2 { margin-top: 0; color: #38bdf8; font-weight: 600; text-align: center; }
                .form-group { margin-bottom: 1.5rem; }
                label { display: block; margin-bottom: 0.5rem; font-size: 0.875rem; color: #94a3b8; }
                input { width: 100%; padding: 0.75rem; background: #0f172a; border: 1px solid #475569; border-radius: 6px; color: #fff; box-sizing: border-box; }
                input:focus { border-color: #7ad0f5; outline: none; }
                button { width: 100%; padding: 0.75rem; background: #0284c7; color: white; border: none; border-radius: 6px; font-weight: 600; cursor: pointer; transition: background 0.2s; }
                button:hover { background: #0369a1; }
                .error-msg { color: #f87171; font-size: 0.875rem; text-align: center; margin-top: 1rem; display: none; }
            </style>
        </head>
        <body>
            <div class="login-card">
                <h2>GritShield Engine</h2>
                <form id="loginForm">
                    <div class="form-group">
                        <label for="username">Administrative Username</label>
                        <input type="text" id="username" name="username" required autocomplete="off">
                    </div>
                    <div class="form-group">
                        <label for="password">Security Password</label>
                        <input type="password" id="password" name="password" required>
                    </div>
                    <button type="submit">Access Control Center</button>
                    <div id="error" class="error-msg"></div>
                </form>
            </div>
            <script>
                document.getElementById('loginForm').addEventListener('submit', async (e) => {
                    e.preventDefault();
                    const username = document.getElementById('username').value;
                    const password = document.getElementById('password').value;
                    const errorDiv = document.getElementById('error');
                    
                    try {
                        const res = await fetch('/admin/api/login', {
                            method: 'POST',
                            headers: { 'Content-Type': 'application/json' },
                            body: JSON.stringify({ username, password })
                        });
                        const data = await res.json();
                        if (data.status === 'success') {
                            window.location.href = '/admin/dashboard';
                        } else {
                            errorDiv.innerText = data.message || 'Invalid administrative credentials';
                            errorDiv.style.display = 'block';
                        }
                    } catch (err) {
                        errorDiv.innerText = 'Network error occurred connecting to framework subsystem.';
                        errorDiv.style.display = 'block';
                    }
                });
            </script>
        </body>
        </html>
    "#;

    Response::ok(login_html.to_string())
}

/// Processes API authorization attempts securely
pub async fn handle_login_auth(ctx: RequestContext) -> Response {
    // Unpack incoming JSON payload
    let auth_data = match ctx.json_body().await {
        Some(data) => data,
        None => {
            return Response::json_ok(&json!({
                "status": "error",
                "message": "Invalid payload format"
            }));
        }
    };

    let input_user = auth_data
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let input_pass = auth_data
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Perform Credential Validation Checks (Static, In-Memory Cached Check)
    let admin = get_admin_credentials();

    if input_user != admin.username || input_pass != admin.password {
        return Response::json(
            HttpStatus::Ok,
            &json!({
                "status": "error",
                "message": "Access Denied: Invalid security signature identifiers provided"
            }),
        );
    }

    let new_sid = uuid::Uuid::new_v4().to_string();

    // Construct the tracking framework session structure
    let mut session_data = std::collections::HashMap::new();
    session_data.insert("admin_user_id".to_string(), "admin_user_1".to_string());

    let new_session = Arc::new(Mutex::new(Session {
        id: new_sid.clone(),
        data: session_data,
        user_id: Some("admin_user_1".to_string()),
        last_accessed: std::time::Instant::now(),
    }));

    // Fetch the shared global store reference and insert the session directly
    let store = get_session_store();
    store.sessions.insert(new_sid.clone(), new_session);

    // Drop a secure signed cookie into the context jar matching the middleware identifier key
    let is_production = crate::core::env::get_env("APP_ENV", "development") == "production";
    let session_cookie = Cookie::new("GASESSION_ID", &new_sid)
        .set_secure(is_production)
        .set_same_site(SameSite::Lax);

    let response = Response::json(
        HttpStatus::Accepted,
        &json!({
            "status": "success",
            "message": "Subsystem authorization handshake completed successfully"
        }),
    );

    ctx.set_signed_cookie(session_cookie);
    response
}

/// Processes API administration logouts securely by tearing down tracking session records
pub async fn handle_logout(ctx: RequestContext) -> Response {
    // If an active tracking cookie exists, clean it out of the shared global memory pool
    if let Some(sid) = ctx.get_signed_cookie("GASESSION_ID") {
        let store = get_session_store();
        store.sessions.remove(&sid);
        debug!(
            "[ADMIN LOGOUT] ✓ Session evicted securely from memory store: {}",
            sid
        );
    }

    // Create an explicitly expired tombstone cookie to instruct the client to purge it
    let _clear_cookie = ctx.remove_cookie("GASESSION_ID");

    Response::redirect(303, "/admin/login")
}

/// Queries the master session pool directly using the administrative cookie jar key
pub fn verify_and_get_admin_id(ctx: &RequestContext) -> Option<String> {
    // Read the unique admin tracking cookie directly from the context jar
    if let Some(admin_sid) = ctx.get_signed_cookie("GASESSION_ID") {
        debug!(
            "[STORE SEARCH] Found GASESSION_ID cookie value: {}",
            admin_sid
        );

        // Fetch the global memory master store and search it directly — no lock step
        let store = get_session_store();
        if let Some(session_ref) = store.sessions.get(&admin_sid) {
            // Lock the individual session instance to inspect its inner variables
            if let Ok(session) = session_ref.value().lock() {
                debug!(
                    "[STORE SEARCH] ✓ Match found in global store! Session state: id={}, user_id={:?}, data={:?}",
                    session.id, session.user_id, session.data
                );

                // Explicitly pull out the admin authorization key
                if let Some(admin_user_id) = session.data.get("admin_user_id") {
                    return Some(admin_user_id.clone());
                } else {
                    warn!("[STORE SEARCH] ✗ Session found, but it lacks the 'admin_user_id' key.");
                }
            }
        } else {
            warn!("[STORE SEARCH] ✗ Cookie token exists, but no matching session is registered in memory pool.");
        }
    } else {
        debug!("[STORE SEARCH] No GASESSION_ID cookie found on incoming request context.");
    }

    None
}

pub struct AdminAuthMiddleware {
    pub store: Arc<SessionStore>,
}

impl AdminAuthMiddleware {
    pub fn new() -> Self {
        Self {
            // Bind the middleware instance to the exact same global store instance
            store: Arc::clone(get_session_store()),
        }
    }
}

impl Middleware for AdminAuthMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Narrow scope to admin routes
        if !ctx.req.path.starts_with("/admin") {
            return MiddlewareResult::Next(None);
        }

        // Skip auth check for login endpoints
        if ctx.req.path == "/admin/login" || ctx.req.path == "/admin/api/login" {
            return MiddlewareResult::Next(None);
        }

        // Search the global store directly bypassing ctx.session
        let mut is_authorized_admin = false;

        if let Some(sid) = ctx.get_signed_cookie("GASESSION_ID") {
            if let Some(session_ptr) = self.store.sessions.get(&sid) {
                if let Ok(session) = session_ptr.value().lock() {
                    if session.data.contains_key("admin_user_id") {
                        is_authorized_admin = true;
                    }
                }
            }
        }

        if is_authorized_admin {
            debug!("[ADMIN AUTH] ✓ Direct store authentication check passed.");
            // Pass Next(None) so we don't return a state that overwrites ctx.session in your connection runner
            MiddlewareResult::Next(None)
        } else {
            debug!(
                "[ADMIN AUTH] ✗ Unauthorized attempt blocked for path: {}",
                ctx.req.path
            );
            if ctx.req.path.starts_with("/admin/api/") {
                MiddlewareResult::Error(Response::unauthorized("Administrative access required"))
            } else {
                MiddlewareResult::Error(Response::redirect(303, "/admin/login"))
            }
        }
    }
}
