//! OpenAPI / Swagger - GritShield developer guide.
//!
//! Run it:
//!
//! ```text
//! cargo run --manifest-path examples/openapi_swagger/Cargo.toml
//! ```
//!
//! Then open <http://127.0.0.1:8083/admin/docs>.
//!
//! ## What is the actual trick?
//!
//! There isn't one, and that is the point. With the `swagger` feature on,
//! `Router::new()` mounts two extra routes:
//!
//! - `GET /admin/docs` - Swagger UI
//! - `GET /admin/docs/openapi.json` - the spec itself
//!
//! Both are generated at request time from two registries that your existing
//! code already populated: the route inventory filled in by `#[get]` /
//! `#[post]` / `#[controller]`, and the schema registry filled in by
//! `GritSchema` and `GritModel`. Nothing in this file describes a path, a
//! method or a field. Delete the whole controller and the endpoints vanish
//! from the spec, because the spec is a projection of the code rather than a
//! second copy of it.
//!
//! Note that no `admin` feature is required for this to work. Add `admin` and
//! the same two paths gain per-model endpoints - and, because the admin auth
//! middleware gates everything under `/admin`, your docs go behind a login.

use gritshield::http::response::HttpStatus;
use gritshield::core::logger::LogLevel;
use gritshield::prelude::*;
use gritshield::{GritSanitizer, GritSchema};
use serde::{Deserialize, Serialize};

/// A request body.
///
/// `GritSchema` is what puts `email`, `name` and `nickname` into the spec. It
/// registers the struct under its own name at startup, and the route below
/// names that registration with `body = CreateUser`.
///
/// `GritSanitizer` is a separate concern that happens to be required here:
/// `ctx.json::<T>()` only accepts a type that implements `GritSanitizable`, so
/// a body you want to deserialise needs it too. It is not documentation.
///
/// The type mapping in the spec is deliberately blunt - `String` stays a string,
/// the integer widths collapse to `i64`, anything unrecognised becomes a string
/// - so read the generated spec as a shape, not as a contract you can validate
/// against. `Option<T>` is the one thing it does carry across: it marks the
/// field nullable.
#[derive(GritSchema, GritSanitizer, Serialize, Deserialize, Debug)]
pub struct CreateUser {
    pub email: String,
    pub name: String,
    pub nickname: Option<String>,
}

pub struct UserController;

/// `GET /api/v1/users` - no parameters, so the operation carries none.
#[controller("/api/v1")]
impl UserController {
    #[get("/users")]
    pub async fn list_users(_ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &serde_json::json!([
                { "id": 1, "email": "ada@example.com", "name": "Ada" },
                { "id": 2, "email": "grace@example.com", "name": "Grace" }
            ]),
        )
    }

    /// `GET /api/v1/users/:id` - the `:id` segment is rewritten to `{id}` and
    /// emitted as a required path parameter.
    #[get("/users/:id")]
    pub async fn get_user(ctx: RequestContext) -> Response {
        let id = ctx.param("id").unwrap_or("0");

        // A path parameter is untrusted input like any other. `param` hands
        // back a plain `&str`, so it is on you to parse or escape it - here,
        // falling back rather than rendering the raw segment into JSON.
        let id: i64 = match id.parse() {
            Ok(id) => id,
            Err(_) => {
                return Response::json(
                    HttpStatus::BadRequest,
                    &serde_json::json!({ "error": "id must be an integer" }),
                )
            }
        };

        Response::json(
            HttpStatus::Ok,
            &serde_json::json!({ "id": id, "email": "ada@example.com", "name": "Ada" }),
        )
    }

    /// `POST /api/v1/users` - `body = CreateUser` is the only annotation this
    /// endpoint needs to acquire a documented request body.
    #[post("/users", body = CreateUser)]
    pub async fn create_user(ctx: RequestContext) -> Response {
        // The same schema does double duty: it is the spec's description of the
        // body *and* the type `ctx.json` deserialises into, so the two cannot
        // drift apart.
        let payload: CreateUser = match ctx.json().await {
            Ok(user) => user,
            // Hand the failure to the framework's error handler rather than
            // stringifying it: `ShieldError` is `Debug` but neither `Display`
            // nor `Serialize`, and it knows its own status code. This is also
            // the path that honours any `catch!` you registered for that code.
            // See the routing example for the full error flow.
            Err(e) => {
                return gritshield::security::errors::default_framework_error_handler(ctx, e)
                    .await;
            }
        };

        Response::json(
            HttpStatus::Created,
            &serde_json::json!({ "id": 3, "email": payload.email, "name": payload.name }),
        )
    }

    /// `DELETE /api/v1/users/:id` - the verb alone changes which field of the
    /// path item is filled in; the path and its parameter are shared with the
    /// `GET` above.
    #[delete("/users/:id")]
    pub async fn delete_user(ctx: RequestContext) -> Response {
        let id = ctx.param("id").unwrap_or("0");
        Response::ok(format!("deleted {id}"))
    }
}

#[tokio::main]
async fn main() {
    let router = Router::new().mount_logger(LogLevel::Trace);

    // Port 8083, continuing the series: security 8080, routing 8081,
    // admin_panel 8082.
    gritshield::http::server::ignite("127.0.0.1", "8083", router).await;
}