//! GritShield admin panel -- developer guide.
//!
//! Run it:
//!
//! ```text
//! cargo run --manifest-path examples/admin_panel/Cargo.toml
//! ```
//!
//! Then sign in at <http://127.0.0.1:8082/admin/login> and you are in the panel.
//!
//! ## How much wiring is there?
//!
//! Less than you would expect. `Router::new()` scans the `inventory` registry
//! and, because the `admin` cargo feature is on, it:
//!
//! 1. mounts the static panel routes (`/admin/dashboard`, `/admin/login`,
//!    `/admin/api/login`, `/admin/api/logout`, `/admin/metrics`,
//!    `/admin/settings/security`, `/admin/rbac-matrix`, `/admin/api/metrics`,
//!    `/admin/api/search-palette`, `/admin/api/create-table`,
//!    `/admin/api/alter-table/:table_slug/add-column`),
//! 2. mounts eight CRUD endpoints per registered model, and
//! 3. installs `AdminAuthMiddleware`, which gates everything under `/admin`.
//!
//! There is no `mount_admin()` to call. Enabling the feature *is* the wiring.
//! `router.rs:72` is the whole of it.
//!
//! That third point has a consequence worth stating plainly: the middleware is
//! pushed onto the **global** middleware stack, so it executes on every request
//! your application serves. It checks `ctx.req.path.starts_with("/admin")` and
//! returns `Next` immediately otherwise, so application routes are unaffected
//! -- but if you were counting middleware layers, enabling `admin` adds one.

mod models;
mod repositories;

use gritshield::core::logger::LogLevel;
use gritshield::database::db::{ensure_internal_audit_log_table, DbConfig, DbManager};
use gritshield::prelude::*;

use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

/// The product table.
///
/// Written as raw SQL rather than a migration file on purpose: the point of
/// this example is the admin panel, and this is the smallest thing that makes
/// it show rows. Note the columns line up with `grid_columns` in
/// `repositories/product.rs` plus the two that stay hidden (`internal_notes`,
/// and `id`, which is the primary key and is never editable).
const CREATE_PRODUCTS: &str = r#"
CREATE TABLE IF NOT EXISTS products (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    sku            TEXT    NOT NULL,
    name           TEXT    NOT NULL,
    price_cents    INTEGER NOT NULL,
    internal_notes TEXT,
    created_at     TEXT    NOT NULL
)
"#;

#[tokio::main]
async fn main() {
    // `DbConfig::default()` reads DATABASE_URL. Left unset, `DbManager::connect`
    // falls back to `sqlite::memory:` and logs a warning -- which is why this
    // example runs with no configuration. An in-memory database is empty on
    // every start, so the schema and the seed data below are not optional.
    let db = DbManager::connect(DbConfig::default())
        .await
        .expect("failed to connect to the database");

    create_schema(&db).await;
    seed(&db).await;

    let router = Router::new()
        .mount_logger(LogLevel::Info)
        // The one piece of wiring that is genuinely required. Without it
        // `ctx.db` is `None` and every panel handler that reads a row hits
        // `.expect("DB connection missing")` -- a panic, not a 500.
        .mount_db(db.clone())
        // An ordinary application route, to make the point that the admin
        // middleware leaves the rest of the app alone.
        .route((
            "/health",
            HttpMethod::GET,
            |_ctx: RequestContext| async move { Response::ok("ok".to_string()) },
        ));

    // Port 8082, so this can run alongside examples/security (8080) and
    // examples/routing (8081).
    gritshield::http::server::ignite("127.0.0.1", "8082", router).await;
}

async fn create_schema(db: &DatabaseConnection) {
    db.execute_unprepared(CREATE_PRODUCTS)
        .await
        .expect("failed to create the products table");

    // Every edit and delete the panel performs writes an `audit_logs` row with
    // the before/after JSON. That table is created by the framework, but it is
    // not created for you -- skip this and the grid appears to work right up
    // until the first save, which then fails.
    ensure_internal_audit_log_table(db)
        .await
        .unwrap_or_else(|e| panic!("failed to create the audit_logs table: {e}"));
}

/// A few rows so the grid has something in it.
async fn seed(db: &DatabaseConnection) {
    // Counted with raw SQL rather than the `products::Entity` so this function
    // does not need the entity in scope, which keeps `main.rs` free of the
    // module that the repository derive is the whole point of.
    let count_stmt = Statement::from_string(
        db.get_database_backend(),
        "SELECT COUNT(*) AS row_count FROM products".to_string(),
    );
    let existing = db
        .query_one(count_stmt)
        .await
        .ok()
        .flatten()
        .and_then(|row| row.try_get::<i64>("", "row_count").ok())
        .unwrap_or(0);

    if existing > 0 {
        return;
    }

    // `internal_notes` is seeded but kept out of `grid_columns`, which is how
    // the example demonstrates a field that is stored and never shown.
    db.execute_unprepared(
        r#"INSERT INTO products (sku, name, price_cents, internal_notes, created_at)
           VALUES
             ('BOLT-001', 'Hex bolt M8',      125, 'supplier A, unit price', '2026-01-04 09:00:00'),
             ('NUT-008', 'Hex nut M8',        35, NULL,                     '2026-01-04 09:05:00'),
             ('WSHR-22','Flat washer M8',     18, 'bulk rate applies',       '2026-01-05 14:30:00'),
             ('SCR-104','Wood screw 4x40',     22, NULL,                     '2026-01-06 08:15:00')
        "#,
    )
    .await
    .expect("failed to seed products");
}