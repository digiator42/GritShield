use gritshield::protocol::response::JsonPayload;
use gritshield::{component, prelude::*};
use gritshield::{scontroller, GritComponent};
use serde_json::json;

use crate::PaymentService;

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

// 2. STRICT LOMBOK WAY: Structs that purely consume other managed dependencies
//    need NO manual constructors and NO `#[component]` impl tags!
#[derive(GritComponent)]
pub struct OrderController {
    pub db: Arc<DatabasePool>, // <-- Auto-resolved from container context
    pub ps: Arc<PaymentService>,
}

#[scontroller("/api/orders")]
impl OrderController {
    #[get("/checkout")]
    pub async fn checkout(&self, ctx: RequestContext) -> Response {
        self.db.execute("SELECT 1").await;
        Response::ok(format!("Checked out safely, {}", self.ps.api_key))
    }
}
