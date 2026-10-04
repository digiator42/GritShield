//! # GritShield routing developer guide
//!
//! ```
//! src/main.rs       pipeline order, manual registration, route tree dump
//! src/basics.rs     controllers, path params, priority, verbs, query strings
//! src/middleware.rs writing the Middleware trait and AfterRequestHook
//! src/errors.rs     custom 404 / 405 pages with #[catch]
//! ```
//!
//! Run with:
//!
//! ```bash
//! cargo run --manifest-path examples/routing/Cargo.toml
//! ```

use gritshield::http::response::HttpStatus;
use gritshield::http::HttpMethod;
use gritshield::prelude::*;
use serde_json::json;
use std::time::Duration;

mod basics;
mod errors;
mod middleware;

use middleware::{ApiKeyMiddleware, AuditLogHook, RequestIdMiddleware, SlowRequestNotifier};

#[tokio::main]
async fn main() {
    // Middleware runs in registration order and every layer can reject, so the
    // cheap and the broad go first. Identical reasoning to the security guide:
    // if the API key check ran last, an unauthenticated flood would still cost
    // you a request-id allocation on every request.
    //
    // `add_middleware` and `add_after_hook` consume and return `Self`, so they
    // chain. `add_route` takes `&mut self` and returns nothing, so it is a
    // statement — which is why the builder has to be `mut`.
    let mut router = Router::new()
        .add_middleware(RequestIdMiddleware::new())
        .add_middleware(ApiKeyMiddleware::new("dev-key", "/secure"))
        // Before-hooks are done; after-hooks observe the finished response.
        .add_after_hook(AuditLogHook)
        .add_after_hook(SlowRequestNotifier {
            threshold: Duration::from_millis(250),
        });

    // One route registered by hand, because `add_route` is a real option and not
    // every endpoint deserves a macro.
    //
    // The fourth argument is the role required to reach it — `None` means
    // anyone. It is wired to the same check as `required_role = ".."` on a route
    // macro, which is the short version of `examples/rbac_caps`.
    router.add_route(
        HttpMethod::GET,
        "/manual",
        |_ctx| async { Response::json(HttpStatus::Ok, &json!({ "registered": "by hand" })) },
        None,
    );

    // The inventory scan that `Router::new()` already performed is worth seeing
    // once. Printing the trie makes the "exact beats :param" rule concrete:
    // `/api/users` ends up with both a `profile` child and an `:id` child.
    router.debug_dump_tree();

    ignite("127.0.0.1", "8081", router).await;
}