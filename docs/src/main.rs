use gritshield::{
    core::env::get_env,
    futures::future::{Ready, ready},
    prelude::*,
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
    let router = Router::new()
        .mount_logger()
        .mount_file_routes("src/pages")
        .expect("");

    println!("[GRITSHIELD] Booting engine cluster...");
    gritshield::core::server::run_server("127.0.0.1", "8080", router, true).await;
}
