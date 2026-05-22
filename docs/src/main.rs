use gritshield::{
    core::env::get_env,
    futures::future::{Ready, ready},
    prelude::*,
    protocol::request::HttpMethod,
    routing::trie::{BoxedResponse, IntoResponse},
    security::errors::FrameworkError,
};

mod pages {
    #[path = "docs/[..path].rs"]
    pub mod docs_wildcard;
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

    router.add_route(HttpMethod::GET, "/health", |_: RequestContext| async move {
        "OK"
    });

    println!("[GRITSHIELD] Booting engine cluster...");
    gritshield::core::server::run_server("0.0.0.0", "8080", router, false).await;
}
