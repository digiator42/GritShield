use crate::protocol::response::Cookie;
use crate::protocol::response::{JsonPayload, SameSite};
use crate::routing::trie::RequestContext;
use crate::security::middleware::{Middleware, MiddlewareResult, MiddlewareState};
use crate::security::session::{Session, SessionStore};
use crate::{debug, prelude::*};
use serde_json::json;
use std::sync::{Mutex, OnceLock};

/// Renders the secure administrative login viewport
pub async fn render_login_page(ctx: RequestContext) -> Response {
    // If the administrator is already authenticated, bypass login and redirect straight to the dashboard
    if let Some(cookie_header) = ctx.req.headers.get("Cookie") {
        if cookie_header.contains("gritshield_admin_session=") {
            return Response::redirect(303, "/admin/dashboard");
        }
    }

    // Render a pristine, framework-branded CSS login page using raw HTML or Maud template formatting
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

use crate::security::xss::Sanitizer;

// Declare a globally accessible, thread-safe cell for the administrative session store
pub static ADMIN_SESSION_STORE: OnceLock<Arc<SessionStore>> = OnceLock::new();

/// Global helper to retrieve or safely initialize the shared admin session memory pool
pub fn get_admin_store() -> &'static Arc<SessionStore> {
    ADMIN_SESSION_STORE.get_or_init(|| Arc::new(SessionStore::new()))
}

/// Processes API authorization attempts securely
pub async fn handle_login_auth(ctx: RequestContext) -> Response {
    // Unpack incoming JSON payload
    let auth_data = ctx.json_body().await.unwrap();

    let input_user = auth_data
        .get("username")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let input_pass = auth_data
        .get("password")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Perform Credential Validation Checks
    let env_admin_user = crate::core::env::get_env("GRITSHIELD_ADMIN_USER", "admin");
    let env_admin_pass = crate::core::env::get_env("GRITSHIELD_ADMIN_PASSWORD", "gritshield2026");

    if input_user != env_admin_user || input_pass != env_admin_pass {
        return Response::json(
            200,
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

    // Fetch the shared global store reference and lock it to insert the session
    let store = get_admin_store();
    if let Ok(mut store_guard) = store.sessions.lock() {
        store_guard.insert(new_sid.clone(), new_session);
    } else {
        return Response::new(
            500,
            Sanitizer::trust("<h1>500 Internal Server Error: Lock Poisoned</h1>"),
        );
    }

    // Drop a secure signed cookie into the context jar matching the middleware identifier key
    let is_production = crate::core::env::get_env("APP_ENV", "development") == "production";
    let session_cookie = Cookie::new("gritshield_admin_session", &new_sid)
        .set_secure(is_production)
        .set_same_site(SameSite::Lax);

    let mut response = Response::json(
        200,
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
    if let Some(sid) = ctx.get_signed_cookie("gritshield_admin_session") {
        let store = get_admin_store();
        if let Ok(mut store_guard) = store.sessions.lock() {
            store_guard.remove(&sid);
            debug!(
                "[ADMIN LOGOUT] ✓ Session evicted securely from memory store: {}",
                sid
            );
        }
    }

    // Create an explicitly expired tombstone cookie to instruct the client to purge it
    let clear_cookie = ctx.remove_cookie("gritshield_admin_session");

    Response::redirect(303, "/admin/login")
}

pub struct AdminAuthMiddleware {
    pub store: Arc<SessionStore>,
}

impl AdminAuthMiddleware {
    pub fn new() -> Self {
        Self {
            // SOLUTION: Bind the middleware instance to the exact same global store instance
            store: Arc::clone(get_admin_store()),
        }
    }
}

impl Middleware for AdminAuthMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        //Restrict to /admin, :TODO need to be strictly to admin registery routes
        if !ctx.req.path.starts_with("/admin") {
            return MiddlewareResult::Next(None);
        }

        // Skip auth checks if the user is explicitly heading to the login page/endpoint
        if ctx.req.path == "/admin/login" || ctx.req.path == "/admin/api/login" {
            return MiddlewareResult::Next(None);
        }

        // Extract session via signed cookie jar and locate it within the master store
        let mut active_session = None;

        if let Some(sid) = ctx.get_signed_cookie("gritshield_admin_session") {
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

            if let Some(session_ptr) = store_guard.get(&sid) {
                debug!("[ADMIN AUTH] ✓ Valid administrative session found: {}", sid);
                active_session = Some(Arc::clone(&session_ptr));
            } else {
                debug!(
                    "[ADMIN AUTH] ✗ Session ID found in cookie but missing from store: {}",
                    sid
                );
            };
        }

        // Reject unauthorized access attempts or unassigned admin privileges
        let session_arc = match active_session {
            Some(s) => s,
            None => {
                debug!(
                    "[ADMIN AUTH] Unauthenticated access attempt blocked for path: {}",
                    ctx.req.path
                );
                if ctx.req.path.starts_with("/admin/api/") {
                    return MiddlewareResult::Error(Response::unauthorized(
                        "Administrative access required",
                    ));
                } else {
                    return MiddlewareResult::Error(Response::redirect(303, "/admin/login"));
                }
            }
        };

        // Verify that this session actually contains authorized administrative credentials
        let admin_user_id = {
            let session = session_arc.lock().unwrap();
            match session.data.get("admin_user_id").cloned() {
                Some(uid) => uid,
                None => {
                    debug!(
                        "[ADMIN AUTH] Session exists but lacks administrative authorization keys."
                    );
                    if ctx.req.path.starts_with("/admin/api/") {
                        return MiddlewareResult::Error(Response::unauthorized(
                            "Administrative access required",
                        ));
                    } else {
                        return MiddlewareResult::Error(Response::redirect(303, "/admin/login"));
                    }
                }
            }
        };

        // Inject metadata into context and bind active session pointer to request lifecycle
        ctx.login_user_id(&admin_user_id);
        ctx.session = Some(Arc::clone(&session_arc));

        // Pass packed state down to downstream controllers seamlessly
        MiddlewareResult::Next(Some(MiddlewareState {
            session: Some(session_arc),
            claims: None,
            session_was_stale: false,
        }))
    }
}
