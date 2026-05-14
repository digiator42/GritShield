use std::sync::Arc;

use gritshield::migration::src::lib::Migrator;
use gritshield::migration::src::lib::MigratorTrait;
use gritshield::security::middleware::{LoggerMiddleware, SessionMiddleware};
use gritshield::security::session::SessionStore;
use gritshield::{
    core::server::run_server,
    routing::trie::Router,
    security::{jwt::JwtHandler, middleware::AuthMiddleware},
};
use sea_orm::{Database, DatabaseConnection};

#[tokio::main]
async fn main() {
    // Connect to the database (creates gritshield.db if it doesn't exist)
    let db: DatabaseConnection = Database::connect("sqlite://gritshield.db?mode=rwc")
        .await
        .expect("Failed to connect to database");

    let mut router = Router::new();

    let shared_db = std::sync::Arc::new(db);

    Migrator::up(&*shared_db, None)
        .await
        .expect("Migration failed!");

    router.add_middleware(LoggerMiddleware);

    let session_store = Arc::new(SessionStore::new());

    router.add_middleware(SessionMiddleware {
        store: Arc::clone(&session_store),
    });

    // Define public routes
    let public_routes = vec![
        "/".to_string(),
        "/products".to_string(),
        "/login".to_string(),
        "/static/".to_string(),
        "/dashboard".to_string(),
        "/home".to_string(),
        "/upload".to_string(),
    ];

    let jwt_kernel = JwtHandler::new("super_secret_key_123");

    // Initialize Auth with the whitelist
    router.add_middleware(AuthMiddleware {
        jwt_handler: jwt_kernel,
        public_paths: public_routes,
    });

    // Register handlers
    // router.add_route(HttpMethod::GET, "/products", products_handler); // PUBLIC
    // router.add_route(HttpMethod::GET, "/static/:*path", static_handler); // PUBLIC
    // router.add_route(HttpMethod::GET, "/dashboard", dashboard_handler); // PUBLIC
    // router.add_route(HttpMethod::GET, "/home", home_handler); // PUBLIC
    // router.add_route(HttpMethod::POST, "/upload", handle_upload); // PUBLIC
    // router.add_route(HttpMethod::GET, "/profile/:name", profile_handler); // PROTECTED

    run_server("127.0.0.1", "8080", router, shared_db, false).await;
}
