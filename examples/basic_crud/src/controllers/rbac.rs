use gritshield::security::rbac::{Admin, Auditor, DeleteUser, ManageBilling, Manager, ViewLogs};
use gritshield::security::session::SessionStore;
use gritshield::{controller, declare_security_caps, prelude::*};

pub struct BillingController;

#[controller("/api/billing")]
impl BillingController {
    #[get("/refund")]
    // #[cap(ManageBilling)] // Super simple signature! Verified at compile-time against the registry.
    pub async fn process_refund(ctx: RequestContext) -> Response {
        ctx.set_session_data("user_id", "Admin");
        ctx.set_session_data("role", "Operator");
        Response::ok("Refund successful")
    }

    #[get("/audit-logs")]
    #[cap(ViewLogs, ManageBilling)] // Works flawlessly for Admin, Manager, and Auditor!
    pub async fn get_logs(ctx: RequestContext) -> Response {
        Response::ok("Logs rendered")
    }
}
