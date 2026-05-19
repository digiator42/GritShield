use std::sync::Arc;

use gritshield::security::middleware::{LoggerMiddleware};
use gritshield::{
    core::server::run_server,
    routing::trie::Router,
    security::{jwt::JwtHandler, middleware::AuthMiddleware},
};
use sea_orm::{Database, DatabaseConnection};

#[tokio::main]
async fn main() {

    let mut router = Router::new();

    router = router.add_middleware(LoggerMiddleware);

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

    let security_middleware = AuthMiddleware::new_session(public_routes);

    // Initialize Auth with the whitelist
    // router.add_middleware(security_middleware);

    // Register handlers
    // router.add_route(HttpMethod::GET, "/products", products_handler); // PUBLIC
    // router.add_route(HttpMethod::GET, "/static/:*path", static_handler); // PUBLIC
    // router.add_route(HttpMethod::GET, "/dashboard", dashboard_handler); // PUBLIC
    // router.add_route(HttpMethod::GET, "/home", home_handler); // PUBLIC
    // router.add_route(HttpMethod::POST, "/upload", handle_upload); // PUBLIC
    // router.add_route(HttpMethod::GET, "/profile/:name", profile_handler); // PROTECTED

    run_server("127.0.0.1", "8080", router, false).await;
}
