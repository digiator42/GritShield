use crate::OrderController;
use gritshield::http::response::JsonPayload;
use gritshield::{component, prelude::*};
use gritshield::{controller, GritComponent};
use serde_json::json;

#[derive(Clone, GritComponent)]
pub struct PrintService {}

#[derive(Clone, GritComponent)]
pub struct DatabasePool {}

// #[component]
impl DatabasePool {
    pub fn new() -> Self {
        DatabasePool {}
    }

    pub async fn execute(&self, str: &str) {
        println!("Executing...");
    }
}

#[derive(GritComponent)]
pub struct InvoiceController {
    pub db: Arc<DatabasePool>, // <-- Auto-resolved from container context
    pub ps: Arc<PrintService>,
}

#[controller("/api/orders")]
impl InvoiceController {
    #[get("/checkout")]
    pub async fn checkout(&self, ctx: RequestContext) -> Response {
        self.db.execute("SELECT 1").await;
        Response::ok(format!("Checked out safely"))
    }

    #[get("/checkout2", role = "Admin")]
    pub async fn checkout2(
        ctx: RequestContext,
        payment: Arc<PrintService>, // <-- Automatically Injected!
    ) -> Response {

        Response::ok(JsonPayload(json!({ "status": "processed" })))
    }
    #[get("/checkout2", role = "Admin")]
    pub async fn checkout3(
        ctx: RequestContext,
        payment: Arc<PrintService>, // <-- Automatically Injected!
    ) -> Response {

        Response::ok(JsonPayload(json!({ "status": "processed" })))
    }
    #[get("/checkout2", role = "Admin")]
    pub async fn checkout4(
        ctx: RequestContext,
        payment: Arc<PrintService>, // <-- Automatically Injected!
    ) -> Response {

        Response::ok(JsonPayload(json!({ "status": "processed" })))
    }
    #[get("/checkout2", role = "Admin")]
    pub async fn checkout5(
        ctx: RequestContext,
        payment: Arc<PrintService>, // <-- Automatically Injected!
    ) -> Response {

        Response::ok(JsonPayload(json!({ "status": "processed" })))
    }
    #[get("/checkout2", role = "Admin")]
    pub async fn checkout6(
        ctx: RequestContext,
        payment: Arc<PrintService>, // <-- Automatically Injected!
    ) -> Response {

        Response::ok(JsonPayload(json!({ "status": "processed" })))
    }
}
