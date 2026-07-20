use crate::OrderController;
use gritshield::http::response::JsonPayload;
use gritshield::routing::engine::ShieldResult;
use gritshield::{component, prelude::*};
use gritshield::{controller, GritComponent, GritSanitizer};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, GritSanitizer)]
pub struct CheckoutRequest {
    #[clean(trim, lowercase)]
    pub email: String,

    #[clean(html_escape, trim)]
    pub note: String,

    pub amount: u64,
}

#[derive(Deserialize, GritSanitizer)]
pub struct Address {
    #[clean(trim, html_escape)]
    pub street: String,
}

#[derive(Deserialize, GritSanitizer)]
pub struct OrderItem {
    #[clean(trim, html_escape)]
    pub title: String,
}

#[derive(Deserialize, GritSanitizer)]
pub struct CreateOrderPayload {
    // 1. Direct struct -> calls Address::sanitize(&mut self.address)
    #[clean(nested)]
    pub address: Address,

    // 2. Option<T> -> calls Option<Address>::sanitize(&mut self.billing_address)
    #[clean(nested)]
    pub billing_address: Option<Address>,

    // 3. Vec<T> -> calls Vec<OrderItem>::sanitize(&mut self.items)
    #[clean(nested)]
    pub items: Vec<OrderItem>,
}

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
    #[get("/checkout-sanitized")]
    pub async fn checkout_sanitized(ctx: RequestContext) -> ShieldResult<Response> {
        // Automatically deserializes & sanitizes email + note in-place
        let payload = ctx.json::<CreateOrderPayload>().await?;

        println!("Cleaned Email: {}", payload.address.street); // Output: trimmed & lowercased
        println!("Safe Note: |{}|", payload.items[0].title); // Output: HTML escaped

        Ok(Response::ok("Checkout complete"))
    }

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
