mod model;
mod root;
mod store;
mod validate;
mod web;

use gritshield::prelude::*;

#[tokio::main]
async fn main() {
    let router = Router::new()
        .route(("/", HttpMethod::GET, web::home))
        .route(("/new", HttpMethod::GET, web::builder_page))
        .route(("/f/:slug", HttpMethod::GET, web::form_page))
        .route(("/stats", HttpMethod::GET, web::stats_page))
        .route(("/theme/toggle", HttpMethod::GET, web::theme_toggle))
        .route(("/api/forms", HttpMethod::GET, web::api_list_forms))
        .route(("/api/forms", HttpMethod::POST, web::api_create_form))
        .route(("/api/forms/:slug", HttpMethod::GET, web::api_get_form))
        .route(("/api/answers/:slug", HttpMethod::POST, web::api_submit_step))
        .add_middleware(web::ApiGuard)
        .add_after_hook(web::StatsHook);

    println!("FormForge listening on http://127.0.0.1:8090");
    ignite("127.0.0.1", "8090", router).await;
}