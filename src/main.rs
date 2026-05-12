use gritshield::utils::dev::profile_handler;
use gritshield::{
    core::server::run_server,
    protocol::request::HttpMethod,
    routing::trie::Router,
    security::{jwt::JwtHandler, middleware::AuthMiddleware},
};

fn main() {
    let mut router = Router::new();

    // Add Auth second (This protects EVERY route)
    let jwt_kernel = JwtHandler::new("a-string-secret-at-least-256-bits-long");
    router.add_middleware(AuthMiddleware {
        jwt_handler: jwt_kernel,
    });

    // Register routes
    router.add_route(HttpMethod::GET, "/profile/:name", profile_handler);

    run_server("127.0.0.1", "8080", router);
}
