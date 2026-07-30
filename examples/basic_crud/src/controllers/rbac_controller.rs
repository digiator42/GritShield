use gritshield::security::rbac::{Admin, Auditor, DeleteUser, Manager, ViewLogs};
use gritshield::security::session::SessionStore;
use gritshield::{controller, prelude::*};
use crate::security::caps::ManageBilling;

pub struct BillingController;

#[controller("/api/billing")]
impl BillingController {
    #[get("/refund")]
    #[cap(ManageBilling)] // Super simple signature! Verified at compile-time against the registry.
    pub async fn process_refund(ctx: RequestContext) -> Response {
        ctx.set_session_data("user_id", "Admin");
        ctx.set_session_data("role", "Operator");
        Response::ok("Refund successful")
    }

    #[get("/audit-logs")]
    #[cap(ManageBilling)] // Works flawlessly for Admin, Manager, and Auditor!
    pub async fn get_logs() -> Response {
        Response::ok("Logs rendered")
    }
}
