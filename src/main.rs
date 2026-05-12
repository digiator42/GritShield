use gritshield::utils::dev::{products_handler, profile_handler};
use gritshield::{
    core::server::run_server,
    protocol::request::HttpMethod,
    routing::trie::Router,
    security::{jwt::JwtHandler, middleware::AuthMiddleware},
};

fn main() {
    let mut router = Router::new();

    // Define public routes
    let public_routes = vec![
        "/".to_string(),
        "/products".to_string(),
        "/login".to_string(),
    ];

    let jwt_kernel = JwtHandler::new("super_secret_key_123");

    // Initialize Auth with the whitelist
    router.add_middleware(AuthMiddleware {
        jwt_handler: jwt_kernel,
        public_paths: public_routes,
    });

    // Register handlers
    router.add_route(HttpMethod::GET, "/products", products_handler); // PUBLIC
    // router.add_route(HttpMethod::GET, "/products", profile_handler); // PUBLIC
    router.add_route(HttpMethod::GET, "/profile/:name", profile_handler); // PROTECTED

    run_server("127.0.0.1", "8080", router);
}
