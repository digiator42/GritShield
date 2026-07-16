use gritshield::{http::request::HttpMethod, prelude::*};

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
    router.add_route(HttpMethod::GET, "/healthz", |_| async move {
        Response::json(200, &"OK")
    });

    gritshield::core::server::ignite("0.0.0.0", "8080", router, false).await;
}
