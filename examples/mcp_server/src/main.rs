//! A worked example of GritShield's native MCP server.
//!
//! Run it:
//!
//! ```text
//! cargo run --manifest-path examples/mcp_server/Cargo.toml
//! ```
//!
//! then, in another terminal:
//!
//! ```text
//! # what can the agent do?
//! curl -s localhost:8080/mcp \
//!   -H 'content-type: application/json' \
//!   -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"curl","version":"1"}}}'
//!
//! curl -s localhost:8080/mcp \
//!   -H 'content-type: application/json' \
//!   -d '{"jsonrpc":"2.0","id":2,"method":"tools/list"}'
//! ```
//!
//! Read `src/tools.rs`, `src/resources.rs` and `src/prompts.rs` in order —
//! they are the guide.

mod prompts;
mod resources;
mod tools;

use gritshield::core::logger::LogLevel;
use gritshield::prelude::*;
use gritshield::database::db::{DbConfig, DbManager};


#[tokio::main]
async fn main() {

    let db_config = DbConfig::default();

    let shared_db = DbManager::connect(db_config).await.unwrap();
    
    // The MCP surface is mounted by `Router::new()`, so there is nothing MCP
    // specific to wire up here. If you need to move it:
    //
    //     MCP_HTTP_PREFIX=/agent cargo run
    //
    // would serve the same transport under `/agent` instead of `/mcp`.

    let router = Router::new()
        .mount_logger(LogLevel::Debug)
        // Hand the pool to the request context. Anything under `/admin` that
        // reads entities - the dashboard included - needs this, otherwise
        // `ctx.db` is `None` and the admin pages fail with a missing
        // database connection.
        .mount_db(shared_db.clone())
        // Add a normal API route alongside the agent surface, if you like.
        .route(("/health", HttpMethod::GET, |_ctx: RequestContext| async move {
            Response::ok("ok".to_string())
        }));

    // While developing you almost certainly want the capability manager:
    //     http://localhost:8080/admin/mcp
    gritshield::http::server::ignite("127.0.0.1", "8080", router).await;
}