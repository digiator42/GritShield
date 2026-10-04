//! Paradigm B: the compile-time container.
//!
//! Nothing here touches the global `CONTEXT`, nothing is discovered at boot,
//! and nothing can be missing - because "missing" is a compile error.

use crate::components::{AppConfig, DatabasePool, PaymentService};
use gritshield::deps::futures::future::FutureExt;
use gritshield::http::response::HttpStatus;
use gritshield::http::request::HttpMethod;
use gritshield::prelude::*;
use gritshield::routing::engine::RequestContext;
use gritshield::{GritWire, WireContainer};
use serde_json::json;
use std::sync::Arc;

/// The container. Every field becomes a `HasComponent<T>` impl, which is what
/// `wire()` demands below. Leave a dependency out of this struct and the
/// controller that needs it stops compiling - see the README.
///
/// Every field has to be an `Arc<T>`. `HasComponent::get_component` returns an
/// `Arc<T>`, and the derive clones the field straight into that return type, so
/// a plain `AppConfig` field does not compile. The *controller*, by contrast,
/// may take either form: a plain field is cloned out of the container's `Arc`.
#[derive(Clone, WireContainer)]
pub struct AppContainer {
    pub db: Arc<DatabasePool>,
    pub payments: Arc<PaymentService>,
    pub config: Arc<AppConfig>,
}

/// The controller. `GritWire` derives `wire<C>(&C) -> Arc<Self>` with a
/// `C: HasComponent<Field>` bound for each field, so the wiring below is
/// type-checked, not reflection.
#[derive(GritWire)]
pub struct CheckoutController {
    pub payments: Arc<PaymentService>,
    pub config: AppConfig,
}

impl CheckoutController {
    /// `config` was cloned out of the container when the controller was built,
    /// not resolved per request.
    pub fn describe(&self) -> String {
        format!(
            "checkout wired at compile time, ceiling {} {}",
            self.config.max_amount_cents, self.config.currency
        )
    }

    pub async fn refund(&self, amount_cents: i64) -> Response {
        match self.payments.charge(amount_cents) {
            Ok(receipt) => Response::json(HttpStatus::Created, &json!({ "refunded": receipt })),
            Err(reason) => {
                Response::json(HttpStatus::UnprocessableEntity, &json!({ "error": reason }))
            }
        }
    }
}

/// Mount the wired controller by hand.
///
/// `.route((path, method, closure))` is the low-level form: nothing is
/// discovered, so the path, the verb and the closure are all spelled out. The
/// closure clones the `Arc` it captured, which is the whole ownership story -
/// one instance, shared by every request.
pub fn mount(
    router: Router,
    container: AppContainer,
) -> Router {
    let checkout = CheckoutController::wire(&container);

    let info = checkout.clone();
    let refunds = checkout.clone();
    router
        .route((
            "/api/strict/info",
            HttpMethod::GET,
            move |_ctx: RequestContext| {
                let info = info.clone();
                async move { Response::ok(info.describe()) }.boxed()
            },
        ))
        .route((
            "/api/strict/refund",
            HttpMethod::POST,
            move |ctx: RequestContext| {
                let refunds = refunds.clone();
                async move {
                    let body: serde_json::Value =
                        ctx.json_body().await.unwrap_or(serde_json::json!({}));
                    let amount = body.get("amount_cents").and_then(|v| v.as_i64()).unwrap_or(0);
                    refunds.refund(amount).await
                }
                .boxed()
            },
        ))
}
