//! Authentication: turning a username and password into a session with a role
//! in it.
//!
//! Authorization in GritShield reads exactly two things off the request: the
//! `user_id` and the `role` stored in the session by `AuthMiddleware`. This
//! module is the only place that decides who gets which role, and it is
//! deliberately boring.

use gritshield::http::response::HttpStatus;
use gritshield::prelude::*;
use gritshield::GritSanitizer;
use serde::{Deserialize, Serialize};

/// A stand-in for your users table: username, password, role.
///
/// Real applications read this from a database and hash the password. What
/// matters for this guide is only the shape - the role string that comes out of
/// here is the one every `role = "..."` and `#[cap(...)]` check is compared
/// against, so it has to match those strings exactly. `Admin` is not `admin`.
const USERS: &[(&str, &str, &str)] = &[
    ("ada", "admin123", "Admin"),
    ("mike", "manager123", "Manager"),
    ("olga", "operator123", "Operator"),
    ("audrey", "auditor123", "Auditor"),
    ("vic", "viewer123", "Viewer"),
];

/// `GritSanitizer` is not optional here: `ctx.json::<T>()` only accepts a
/// `T: GritSanitizable`. It sanitizes the strings as they are deserialized, so
/// a username containing markup is cleaned before it ever reaches the lookup
/// below - which matters more than usual for a login endpoint.
#[derive(GritSanitizer, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResult {
    pub user_id: String,
    pub role: String,
}

/// Public, because `AuthMiddleware` is configured with `/auth/login` in its
/// public path list. Everything else in this example requires a session.
pub struct AuthController;

#[controller("/auth")]
impl AuthController {
    #[post("/login")]
    pub async fn login(ctx: RequestContext) -> Response {
        let creds: Credentials = match ctx.json().await {
            Ok(creds) => creds,
            Err(_) => {
                return Response::json(
                    HttpStatus::BadRequest,
                    &serde_json::json!({ "error": "expected a JSON body with username and password" }),
                )
            }
        };

        let found = USERS.iter().find(|u| {
            u.0 == creds.username && u.1 == creds.password
        });

        let (username, _, role) = match found {
            Some(user) => user,
            None => {
                // One message for "no such user" and "wrong password" on
                // purpose: a different response for each would tell an attacker
                // which usernames exist.
                return Response::json(
                    HttpStatus::Unauthorized,
                    &serde_json::json!({ "error": "invalid username or password" }),
                );
            }
        };

        // Two calls, and both matter. `login_user_id` is what
        // `AuthMiddleware` treats as proof of authentication for every later
        // request; `set_session_data` is what `ctx.has_role` reads. Log in and
        // forget the second and you are authenticated but never authorized.
        ctx.login_user_id(username);
        ctx.set_session_data("role", role);

        Response::json(
            HttpStatus::Ok,
            &LoginResult {
                user_id: username.to_string(),
                role: role.to_string(),
            },
        )
    }

    /// Who am I? Private on purpose, so calling it without a session returns
    /// `401` from the middleware rather than an empty success.
    #[get("/me")]
    pub async fn me(ctx: RequestContext) -> Response {
        let user_id = ctx
            .get_session_data("user_id")
            .unwrap_or_else(|| "unknown".to_string());
        let role = ctx.get_session_data("role");

        Response::json(
            HttpStatus::Ok,
            &serde_json::json!({
                "user_id": user_id,
                "role": role,
                // `get_user_role` is the same lookup `has_role` uses internally:
                // session first, then JWT claims.
                "resolved_role": ctx.get_user_role(),
            }),
        )
    }
}
