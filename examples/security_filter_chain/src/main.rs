//! # GritShield `SecurityFilterChain` developer guide
//!
//! Two chains, eleven routes, no database and no hand-written authorization in
//! the handlers:
//!
//! | Chain | Matcher | Filters | Rules |
//! |-------|---------|---------|-------|
//! | `api` | `/api/**` | JWT bearer | public, Authenticated, Admin role, method-specific |
//! | `web` | `/**` | signed session + CSRF | public, Authenticated, Admin role |
//!
//! The `api` chain is registered first, so `/api/**` never falls through to the
//! catch-all `web` chain. That is the same "first matching chain wins" rule as
//! Spring Security.
//!
//! ## Run
//!
//! ```bash
//! cargo run --manifest-path examples/security_filter_chain/Cargo.toml
//! ```
//!
//! The server prints one Admin and one User bearer token at startup. The demo
//! secret is hard-coded on purpose; read it from configuration in production.
//!
//! ## curl walkthrough
//!
//! ```bash
//! # Public API: no identity needed.
//! curl -i http://127.0.0.1:8090/api/public/ping
//!
//! # A tampered bearer token is rejected even on a public path.
//! curl -i http://127.0.0.1:8090/api/public/ping \
//!   -H "Authorization: Bearer nope"
//!
//! # JWT: replace $ADMIN with the token printed at startup.
//! curl -i http://127.0.0.1:8090/api/me \
//!   -H "Authorization: Bearer $ADMIN"
//! curl -i http://127.0.0.1:8090/api/admin/panel \
//!   -H "Authorization: Bearer $ADMIN"
//!
//! # Session login, then reuse the cookie.
//! curl -i -c cookies.txt -X POST http://127.0.0.1:8090/auth/login
//! curl -i -b cookies.txt http://127.0.0.1:8090/account/me
//! curl -i -b cookies.txt http://127.0.0.1:8090/admin/dashboard
//!
//! # CSRF: read the token, then send it with the state-changing request.
//! curl -s -b cookies.txt http://127.0.0.1:8090/auth/session
//! curl -i -b cookies.txt -X POST http://127.0.0.1:8090/account/update \
//!   -H "X-CSRF-Token: <csrf_token>"
//! ```
//!
//! Anonymous `GET /account/me` answers `303 See Other` to `/auth/login`; a
//! signed-in but non-Admin session gets `403` on `/admin/dashboard`.

use gritshield::deps::async_trait;
use gritshield::http::HttpMethod;
use gritshield::prelude::*;
use gritshield::security::jwt::Claims;
use serde_json::json;

const JWT_SECRET: &str = "demo-only-secret-change-me-in-production";
const ADDRESS: &str = "127.0.0.1";
const PORT: &str = "8090";

/// A custom filter proves both phases are available to application code.
///
/// `on_request` could reject the request (return `FilterResult::Respond`), but
/// this one only stamps the response so the ordering is visible with `curl -i`.
struct ChainStamp;

#[async_trait]
impl SecurityFilter for ChainStamp {
    fn name(&self) -> &'static str {
        "chain-stamp"
    }

    async fn on_request(&self, _ctx: &mut RequestContext) -> FilterResult {
        FilterResult::Continue
    }

    async fn on_response(&self, _ctx: &RequestContext, res: &mut Response) {
        res.headers
            .push(("x-security-chain".to_string(), "web".to_string()));
    }
}

#[tokio::main]
async fn main() {
    let tokens = JwtHandler::new(JWT_SECRET);
    let admin_token = tokens
        .sign(&Claims::new("ada".to_string(), "Admin".to_string(), 3600))
        .expect("demo Admin token");
    let user_token = tokens
        .sign(&Claims::new("bob".to_string(), "User".to_string(), 3600))
        .expect("demo User token");

    let router = Router::new()
        // =====================================================================
        // Public routes
        // =====================================================================
        .route(("/health", HttpMethod::GET, |_ctx: RequestContext| async {
            Response::json_ok(&json!({ "status": "ok" }))
        }))
        .route((
            "/api/public/ping",
            HttpMethod::GET,
            |ctx: RequestContext| async move {
                // Presenting an invalid token is a failed authentication, not
                // an anonymous request; the chain rejects it before this runs.
                let anonymous = ctx.claims.is_none();
                Response::json_ok(&json!({ "pong": true, "anonymous": anonymous }))
            },
        ))
        // =====================================================================
        // Session flow
        // =====================================================================
        .route((
            "/auth/login",
            HttpMethod::POST,
            |ctx: RequestContext| async move {
                ctx.login_user_id("ada");
                ctx.set_session_data("role", "Admin");
                Response::json_ok(&json!({
                    "logged_in": true,
                    "user_id": ctx.get_user_id(),
                    "role": ctx.get_user_role(),
                }))
            },
        ))
        .route((
            "/auth/session",
            HttpMethod::GET,
            |ctx: RequestContext| async move {
                Response::json_ok(&json!({
                    "authenticated": ctx.is_user_authenticated(),
                    "user_id": ctx.get_user_id(),
                    "role": ctx.get_user_role(),
                    "csrf_token": ctx.get_csrf_token(),
                }))
            },
        ))
        .route((
            "/account/me",
            HttpMethod::GET,
            |ctx: RequestContext| async move {
                Response::json_ok(&json!({
                    "user_id": ctx.get_user_id(),
                    "role": ctx.get_user_role(),
                    "source": "session cookie",
                }))
            },
        ))
        .route((
            "/account/update",
            HttpMethod::POST,
            |ctx: RequestContext| async move {
                Response::json_ok(&json!({
                    "updated": true,
                    "user_id": ctx.get_user_id(),
                    "csrf": "verified",
                }))
            },
        ))
        .route((
            "/admin/dashboard",
            HttpMethod::GET,
            |ctx: RequestContext| async move {
                Response::json_ok(&json!({
                    "area": "admin",
                    "user_id": ctx.get_user_id(),
                    "message": "only Admins get this far",
                }))
            },
        ))
        // =====================================================================
        // JWT API
        // =====================================================================
        .route((
            "/api/me",
            HttpMethod::GET,
            |ctx: RequestContext| async move {
                let claims = ctx.claims.as_ref();
                Response::json_ok(&json!({
                    "user_id": claims.map(|c| c.sub.clone()),
                    "role": claims.map(|c| c.role.clone()),
                    "source": "bearer token",
                }))
            },
        ))
        .route((
            "/api/admin/panel",
            HttpMethod::GET,
            |_ctx: RequestContext| async {
                Response::json_ok(&json!({
                    "area": "api-admin",
                    "message": "Admin bearer token accepted",
                }))
            },
        ))
        .route((
            "/api/items",
            HttpMethod::GET,
            |_ctx: RequestContext| async {
                Response::json_ok(&json!({ "items": ["alpha", "beta"] }))
            },
        ))
        .route((
            "/api/items",
            HttpMethod::POST,
            |_ctx: RequestContext| async {
                Response::json_ok(&json!({ "created": true, "items": ["alpha", "beta", "gamma"] }))
            },
        ))
        // =====================================================================
        // API chain: registered first, so it owns every /api/** request.
        // =====================================================================
        .add_security_chain(
            SecurityFilterChain::builder()
                .name("api")
                .matching("/api/**")
                .jwt_auth(JwtHandler::new(JWT_SECRET))
                // Rules are ordered and the first match wins. A public rule
                // does not bypass authentication: a malformed token is still
                // rejected by the filter above.
                .authorize_public("/api/public/**")
                .authorize(
                    RequestMatcher::path_and_method("/api/items", HttpMethod::POST),
                    AccessRule::has_role("Admin"),
                )
                .authorize_role("Admin", "/api/admin/**")
                .any_request(AccessRule::authenticated())
                .entry_point(AuthenticationEntryPoint::Bearer)
                .access_denied(AccessDeniedHandler::Forbidden)
                .build(),
        )
        // =====================================================================
        // Web chain: catches everything the API chain did not claim.
        // =====================================================================
        .add_security_chain(
            SecurityFilterChain::builder()
                .name("web")
                .matching("/**")
                .session_auth()
                .filter(ChainStamp)
                .csrf_with(|csrf| csrf.ignore_path("/auth/login"))
                .authorize_public("/health")
                .authorize_public("/auth/**")
                .authorize_authenticated("/account/**")
                .authorize_role("Admin", "/admin/**")
                // No catch-all rule: any path this chain matches but no rule
                // covers is denied (fail-closed) instead of silently open.
                .entry_point(AuthenticationEntryPoint::redirect("/auth/login"))
                .access_denied(AccessDeniedHandler::Forbidden)
                .build(),
        );

    println!("GritShield security filter chain example");
    println!("  http://{ADDRESS}:{PORT}/health");
    println!();
    println!("Admin bearer token (role Admin):");
    println!("  {admin_token}");
    println!();
    println!("User bearer token (role User):");
    println!("  {user_token}");
    println!();
    println!("Ordering: /api/** is registered before /**; first matching chain wins.");

    ignite(ADDRESS, PORT, router).await;
}
