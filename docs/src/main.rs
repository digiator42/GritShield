use gritshield::{
    prelude::*,
    protocol::request::HttpMethod,
};

mod pages {
    #[path = "docs/[..path].rs"]
    pub mod docs_wildcard;
    #[path = "404.rs"]
    pub mod not_found;
}
mod root;

#[get("/static/:*path")]
async fn static_assets(ctx: RequestContext) -> Response {
    let path = ctx.params.get("*path").unwrap().as_str();

    Response::static_file(&format!("/static/{}", path).as_str())
}

#[tokio::main]
async fn main() {
    let mut router = Router::new()
        .mount_logger()
        .mount_file_routes("src/pages")
        .expect("");

    // Render health check endpoint
    router.add_route(HttpMethod::GET, "/healthz", |_: RequestContext| async move {
        Response::json(200, &"OK")
    });

    println!("[GRITSHIELD] Booting engine cluster...");
    gritshield::core::server::run_server("0.0.0.0", "8080", router, false).await;
}
