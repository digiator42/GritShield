//! Background work - GritShield developer guide.
//!
//! ```text
//! cargo run --manifest-path examples/job_queue/Cargo.toml
//! ```
//!
//! One HTTP route fans out into four kinds of background work - an event with two
//! handlers, three jobs, and a cron job ticking every second - and the whole
//! interleaving is readable over HTTP afterwards:
//!
//! ```text
//! curl -X POST http://127.0.0.1:8086/api/orders \
//!   -H 'Content-Type: application/json' \
//!   -d '{"email":"ada@example.com","total_cents":4200,"succeed_on_attempt":3}'
//! sleep 8
//! curl http://127.0.0.1:8086/api/activity
//! ```
//!
//! Nothing here needs a database. The default storage is an in-process channel,
//! `ignite` starts the workers and the scheduler, and `Router::new()` discovers
//! every `#[event]` handler before the listener binds.

mod activity;
mod api;
mod events;
mod jobs;

use gritshield::core::logger::LogLevel;
use gritshield::http::server::ignite;
use gritshield::prelude::*;

#[tokio::main]
async fn main() {
    activity::record("boot", "job queue guide starting");

    // This one line does the DI boot, registers every route the attribute
    // macros submitted, and - the part that matters here - calls
    // `EventBus::auto_discover()`, which subscribes every `#[event]` handler in
    // the binary to the bus. There is no separate "register my handlers" step.
    let router = Router::new().mount_logger(LogLevel::Info);

    // Inside `ignite`, two background tasks appear: a `JobWorkerEngine` with
    // ten workers polling `router.job_queue`, and a `CronScheduler` walking the
    // cron strings submitted by `#[job(cron = "...")]`. Neither is shut down
    // separately; both end with the process.
    //
    // Port 8086 continues the series: security 8080, routing 8081, admin_panel
    // 8082, openapi_swagger 8083, rbac_caps 8084, dependency_injection 8085.
    ignite("127.0.0.1", "8086", router).await;
}
