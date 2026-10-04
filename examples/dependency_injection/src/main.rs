//! Dependency injection - GritShield developer guide.
//!
//! ```text
//! cargo run --manifest-path examples/dependency_injection/Cargo.toml
//! ```
//!
//! No database, no Redis, no session. Every endpoint below either receives a
//! dependency as a handler argument or was handed one at wire time.
//!
//! ```text
//! curl -X POST http://127.0.0.1:8085/api/billing/charge \
//!   -H 'Content-Type: application/json' -d '{"amount_cents": 4200}'
//! curl http://127.0.0.1:8085/api/billing/graph
//! curl http://127.0.0.1:8085/api/strict/info
//! ```
//!
//! GritShield has two independent DI engines and this example runs both in one
//! binary, on one router, side by side.
//!
//! - **Paradigm A, dynamic.** Annotate a type, and the container discovers it,
//!     builds it, and injects it wherever a handler asks. `main.rs` does not
//!     change when you add a service or a controller.
//! - **Paradigm B, compile-time.** You declare the graph, and the compiler
//!     refuses to build a controller whose container is missing a piece.
//!
//! Neither one is a wrapper around the other. Paradigm A uses a global
//! registry; Paradigm B never touches it.

mod components;
mod dynamic;
mod strict;

use components::AppConfig;
use gritshield::http::server::ignite;
use gritshield::core::logger::LogLevel;
use gritshield::prelude::*;
use gritshield::inject;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    // `mark_injectable!(AppConfig)` lives at module scope in `components.rs`,
    // because it expands to an `impl`. What is left is handing the container a
    // value it could not have built on its own.
    //
    // Order matters here, but not for the reason you would guess.
    // `inventory::submit!` expands to a process constructor, so the *graph
    // entry* for AppConfig exists before `main` starts and `Router::new()`
    // would pass its completeness check even with this call moved further down.
    // What is ordered is the *value*: the boot hooks resolve their dependencies
    // while `Router::new()` runs, so move this below it and boot dies with
    // "Critical Bootstrap DI Fault: Failed to resolve dependency 'AppConfig'
    // required by component 'PaymentService'".
    inject!(
        AppConfig,
        AppConfig {
            currency: "EUR".to_string(),
            max_amount_cents: 100_000,
        }
    );

    // Boot happens inside `Router::new()`, not here: it verifies that every
    // declared dependency has a provider, then runs the registration hooks that
    // build `DatabasePool`, `PaymentService` and `InvoiceService`. A missing
    // component is a panic at this line, naming the component and what it
    // wanted - not a surprise on the first request that needs it.
    let router = Router::new().mount_logger(LogLevel::Info);

    // Paradigm B, wired explicitly. The container is an ordinary struct you
    // build by hand, and `wire()` checks it against `CheckoutController`'s
    // fields at compile time.
    //
    // Note the duplicated `AppConfig`: these are separate instances from the one
    // `inject!` put in the global registry. That is the point - Paradigm B never
    // reads `CONTEXT`, so the two graphs are genuinely independent.
    let container = strict::AppContainer {
        db: Arc::new(components::DatabasePool::new()),
        payments: Arc::new(components::PaymentService::new(
            Arc::new(components::DatabasePool::new()),
            AppConfig {
                currency: "EUR".to_string(),
                max_amount_cents: 100_000,
            },
        )),
        config: Arc::new(AppConfig {
            currency: "EUR".to_string(),
            max_amount_cents: 100_000,
        }),
    };

    let router = strict::mount(router, container);

    // Port 8085, continuing the series: security 8080, routing 8081,
    // admin_panel 8082, openapi_swagger 8083, rbac_caps 8084.
    ignite("127.0.0.1", "8085", router).await;
}
