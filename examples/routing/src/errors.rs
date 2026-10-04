//! Custom error pages with `#[catch]`.
//!
//! Without this, an unmatched path gets a bare `<h1>404 Not Found</h1>`. With a
//! `#[catch]` handler registered for that status, you get your own response.
//!
//! ## How the lookup works
//!
//! ```text
//! no route matched
//!   └─▶ default_framework_error_handler
//!         ├─ is there a #[r#catch(status = 404)]?  ──▶ yes ──▶ call it
//!         └─ otherwise ──▶ built-in HTML error page
//! ```
//!
//! Handlers are stored in a registry keyed by status code, so a `404` and a
//! `405` handler coexist. Two constraints on the function itself: it must be
//! `async`, and it must take a `RequestContext`.
//!
//! ## Two things that bite before it compiles
//!
//! **1. The attribute name needs a raw identifier.** `catch` is a reserved
//! keyword in Rust 2021, so `use gritshield::catch;` does not parse, and the
//! prelude does not re-export it. `r#catch` is the way in:
//!
//! ```rust,ignore
//! use gritshield::r#catch;
//!
//! #[r#catch(status = 404)]
//! ```
//!
//! **2. Your crate needs `ctor` as a direct dependency.** The expansion emits a
//! startup registration guarded by `#[::gritshield::startup::ctor(unsafe)]`,
//! and a `#[ctor]` attribute resolves against the crate being compiled — not
//! against GritShield. Without `ctor = "1.0"` in your `Cargo.toml` this fails
//! with `cannot find 'ctor' in the crate root`, which points nowhere near the
//! real cause. See the note on `ctor` in this example's `Cargo.toml`.
//!
//! A third question comes up often enough to answer here: a `405` handler needs
//! an `HttpStatus`, since `Response::json` takes one and not a number.
//! `HttpStatus::MethodNotAllowed` is the variant for it.
//!
//! ```bash
//! curl -i http://127.0.0.1:8081/no-such-route          # custom 404
//! curl -i http://127.0.0.1:8081/api/items/7            # custom 405
//! ```

use gritshield::http::response::HttpStatus;
use gritshield::prelude::*;
use gritshield::r#catch;
use serde_json::json;

/// Replaces the 404 page for every unmatched path.
///
/// ```bash
/// curl -i http://127.0.0.1:8081/definitely-not-here
/// # HTTP 404, application/json, {"error":"no route","path":"..."}
/// ```
///
/// Responding with JSON rather than HTML is a deliberate choice, and it is the
/// right one for an API. The default error page is an HTML document, which a
/// JSON client will fail to parse; the status code is the part of the contract
/// that matters, and this keeps the body consistent with the rest of the API.
#[r#catch(status = 404)]
pub async fn not_found(ctx: RequestContext) -> Response {
    Response::json(
        HttpStatus::NotFound,
        &json!({
            "error": "no route matched",
            "path": ctx.req.path,
            // `HttpMethod` derives Debug but not Serialize or Display, so the
            // verb reaches JSON via `{:?}` rather than being placed directly.
            "method": format!("{:?}", ctx.req.method),
            "hint": "GET / lists the routes this guide registers",
        }),
    )
}

/// Replaces the 405 page.
///
/// Worth distinguishing from the 404 above: the path *does* exist, the verb is
/// wrong. Saying so saves a caller from concluding the resource is gone.
///
/// ```bash
/// curl -i http://127.0.0.1:8081/api/items/7
/// # HTTP 405, {"error":"wrong verb","allowed":["PUT","PATCH","DELETE"]}
/// ```
///
/// ## A note on the status variant
///
/// `HttpStatus` carries a `MethodNotAllowed = 405` variant precisely so this
/// handler can use `Response::json`. Reaching for a number instead is not an
/// option: `Response::new` takes a `u16` but hardcodes
/// `Content-Type: text/html`, and appending a second `Content-Type` leaves the
/// wrong one first in the list.
///
/// The router already produces this status on its own — a matched path with the
/// wrong verb resolves to `RoutingResult::MethodNotAllowed` before any handler
/// runs. Registering this handler only replaces the body, which is the point of
/// `#[catch]`.
#[r#catch(status = 405)]
pub async fn method_not_allowed(ctx: RequestContext) -> Response {
    Response::json(
        HttpStatus::MethodNotAllowed,
        &json!({
            "error": "wrong verb",
            "path": ctx.req.path,
            "method": format!("{:?}", ctx.req.method),
            "allowed": ["PUT", "PATCH", "DELETE"],
        }),
    )
    // RFC 9110 requires `Allow` on a 405. The default router response sets it,
    // but a `#[catch]` handler replaces the response wholesale, so it is the
    // handler's job now.
    .with_header("Allow", "PUT, PATCH, DELETE")
}