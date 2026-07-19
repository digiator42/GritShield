use gritshield::http::response::JsonPayload;
use gritshield::prelude::*;
use gritshield::GritComponent;
use serde_json::json;

#[derive(GritComponent)]
pub struct TestPaymentService {
    pub secret: Arc<String>,
}

pub struct TestController {
    pub key: Arc<String>,
}

#[controller("")]
impl TestController {
    #[get("/api/orders/checkout/test")]
    pub async fn checkout(
        payment: Arc<TestPaymentService>, // <-- Automatically Injected!
        //   db: Arc<DatabasePool>,        // <-- Automatically Injected!
        // oc: Arc<OrderController>,
    ) -> Response {
        // let order_payload = ctx.json_body().await.unwrap();

        // Use services directly with zero manual orchestration!
        // payment.process_charge(21).await;
        // db.execute("INSERT INTO orders ...").await;

        // let _ = oc.checkout(32).await;

        // oc.payment.process_charge(32).await;
        // let key = oc.payment.api_key.clone();

        // oc.checkout(ctx);

        Response::ok(JsonPayload(json!({ "status": "key" })))
    }
}

// #[get("/api/orders/checkout2")]
// pub async fn checkout(
//     ctx: RequestContext,
//     payment: Arc<PaymentService>, // <-- Automatically Injected!
// ) -> Response {
//     // let order_payload = ctx.json_body().await.unwrap();

//     // Use services directly with zero manual orchestration!
//     payment.process_charge(21);
//     // db.execute("INSERT INTO orders ...").await;

//     Response::ok(JsonPayload(json!({ "status": "processed" })))
// }
