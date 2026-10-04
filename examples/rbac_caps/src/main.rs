//! RBAC and capabilities - GritShield developer guide.
//!
//! ```text
//! cargo run --manifest-path examples/rbac_caps/Cargo.toml
//! ```
//!
//! Then log in as one of five roles and walk the endpoints:
//!
//! ```text
//! curl -c jar -X POST http://127.0.0.1:8084/auth/login \
//!   -H 'Content-Type: application/json' \
//!   -d '{"username":"olga","password":"operator123"}'
//! curl -b jar -i http://127.0.0.1:8084/api/logs
//! ```
//!
//! ## The model in one paragraph
//!
//! Authentication answers "who is this?". `AuthMiddleware` answers it from a
//! session and stores a `user_id` and a `role` string in it. Authorization is
//! then either a role comparison or a capability check, and GritShield offers
//! both. Roles are strings and roles inherit from each other through a tree you
//! declare on the router. Capabilities are types; a macro binds each one to the
//! roles that satisfy it, and the compiler refuses a `#[cap(..)]` that names a
//! capability nobody declared.

mod api;
mod auth;
mod security;

use gritshield::http::response::HttpStatus;
use gritshield::middleware::AuthMiddleware;
use gritshield::prelude::*;
use gritshield::http::server::ignite;
use serde_json::json;

/// Routes reachable without a session.
///
/// Everything not listed here requires `user_id` in the session, and gets a
/// `401` without one.
const PUBLIC_PATHS: &[&str] = &["/", "/api/ping", "/auth/login"];

pub struct IndexController;

/// A landing page, so following a redirect from `/logout` lands somewhere
/// useful instead of a 404.
#[controller("")]
impl IndexController {
    #[get("/")]
    pub async fn index() -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({
                "service": "rbac_caps",
                "users": {
                    "ada": "Admin", "mike": "Manager", "olga": "Operator",
                    "audrey": "Auditor", "vic": "Viewer",
                },
                "passwords": "the username with 123 appended",
                "endpoints": {
                    "public": ["GET /api/ping"],
                    "role attribute": ["GET /api/reports (Auditor)"],
                    "inline guard": ["GET /api/team/roster (Manager)"],
                    "capabilities": [
                        "GET /api/logs (ViewAuditLog: Admin, Manager, Auditor)",
                        "POST /api/refunds (ManageBilling | RefundOrder: Admin, Manager, Operator)",
                        "DELETE /api/accounts/:id (DeleteAccount: Admin)",
                    ],
                },
            }),
        )
    }
}

#[tokio::main]
async fn main() {
    // `new_session` takes the public path list and an optional redirect for
    // unauthenticated requests. The second argument is `None` here on purpose:
    // pass `Some("/auth/login")` and an unauthenticated request to a private
    // endpoint answers `303` instead of `401`, which is right for a browser and
    // wrong for an API client. See the security example for the redirect form.
    //
    // CSRF is off by default here too. Turn it on with `auth.enable_csrf =
    // true` and every POST, PUT, PATCH and DELETE needs the session's
    // `csrf_token` in an `x-csrf-token` header or a `csrf_token` form field.
    // The curl walkthrough in the README would all start failing with `403`,
    // which is the point.
    let auth = AuthMiddleware::new_session(
        PUBLIC_PATHS.iter().map(|p| p.to_string()).collect(),
        None,
    );

    // The inheritance tree. `has_role` walks it recursively, so `Admin` reaches
    // `Viewer` through `Manager`, while `Operator` reaches nothing - `Operator`
    // has no children, and siblings do not inherit from each other.
    let router = Router::new()
        .add_role_inheritance("Admin", vec!["Manager", "Operator", "Auditor"])
        .add_role_inheritance("Manager", vec!["Viewer"])
        .add_middleware(auth);

    // Port 8084, continuing the series: security 8080, routing 8081,
    // admin_panel 8082, openapi_swagger 8083.
    ignite("127.0.0.1", "8084", router).await;
}
