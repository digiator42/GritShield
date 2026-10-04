//! Paradigm A: the dynamic container.
//!
//! Nothing here names `InvoiceService` in a constructor. The controller is
//! discovered through `inventory`, and its collaborators are pulled out of the
//! global `CONTEXT` when the container boots.

use crate::components::{InvoiceService, PaymentService};
use gritshield::http::response::HttpStatus;
use gritshield::prelude::*;
use gritshield::routing::engine::ShieldResult;
use serde_json::json;
use std::sync::Arc;

pub struct BillingController;

/// `#[derive(GritComponent)]` is not needed on the controller: `#[controller]`
/// already submits its dependency edges, and the container hands it over
/// resolved. What the framework does *not* do is store the controller - this
/// one exists only for the length of a request, which is why `&self` works
/// without a container field anywhere.
#[controller("/api/billing")]
impl BillingController {
    /// Handler-argument injection. `Arc<PaymentService>` as a second parameter
    /// is resolved from the container by the route macro itself, so the handler
    /// body never mentions where it came from.
    #[post("/charge")]
    pub async fn charge(
        ctx: RequestContext,
        payments: Arc<PaymentService>,
    ) -> ShieldResult<Response> {
        let body: serde_json::Value = ctx.json_body().await.unwrap_or(json!({}));
        let amount = body.get("amount_cents").and_then(|v| v.as_i64()).unwrap_or(0);

        match payments.charge(amount) {
            Ok(receipt) => Ok(Response::json(
                HttpStatus::Created,
                &json!({ "receipt": receipt }),
            )),
            Err(reason) => Ok(Response::json(
                HttpStatus::UnprocessableEntity,
                &json!({ "error": reason }),
            )),
        }
    }

    /// Field injection, spelled as a handler argument again. `InvoiceService` is
    /// a `GritComponent`, so the container built it from its fields at boot.
    #[get("/invoice/:customer")]
    pub async fn invoice(
        ctx: RequestContext,
        invoices: Arc<InvoiceService>,
    ) -> Response {
        let customer = ctx.param("customer").unwrap_or("anonymous");
        Response::json(
            HttpStatus::Ok,
            &json!({ "document": invoices.render(customer) }),
        )
    }

    /// The graph the container built, straight from the registry inventory.
    /// Useful when a component is not behaving and you need to know what the
    /// container thinks exists.
    #[get("/graph")]
    pub async fn graph() -> Response {
        Response::ok(gritshield::core::ioc::AutoWire::export_mermaid())
    }
}
