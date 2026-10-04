//! The endpoints under test.
//!
//! Four different authorization mechanisms, one file:
//!
//! - a public route, to show what "no authorization" looks like;
//! - `role = "Auditor"`, enforced centrally before the handler runs;
//! - `ctx.require_role(..)?`, enforced by the handler itself;
//! - `#[cap(..)]`, enforced by generated code in front of the handler.

use crate::security::{DeleteAccount, ManageBilling, RefundOrder, ViewAuditLog};
use gritshield::http::response::HttpStatus;
use gritshield::prelude::*;
use gritshield::routing::engine::ShieldResult;
use serde_json::json;

pub struct ApiController;

#[controller("/api")]
impl ApiController {
    /// No `role`, no `#[cap]`, and listed as public in `main.rs`. This is what
    /// a route with no authorization looks like: the middleware lets it past
    /// because it is public, and the framework has no opinion about it.
    #[get("/ping")]
    pub async fn ping() -> Response {
        Response::json(HttpStatus::Ok, &json!({ "pong": true }))
    }

    /// The `role` attribute. The check runs in the connection loop *before* the
    /// handler is called, so a handler cannot forget it - and, just as
    /// importantly, the handler never learns that it was called.
    ///
    /// Requires `Auditor`, which means: Auditor yes, Admin yes (inherits it),
    /// Manager no, Operator no, Viewer no.
    #[get("/reports", role = "Auditor")]
    pub async fn reports(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({
                "access": "granted",
                "by": "role attribute",
                "role": ctx.get_user_role(),
                "reports": ["monthly-revenue", "churn", "uptime"],
            }),
        )
    }

    /// The same check, written by hand. `require_role` returns
    /// `ShieldResult<()>`, so `?` turns a failure into `403 Forbidden` and
    /// keeps the happy path flat.
    ///
    /// Use this when the requirement is conditional - when the guard depends on
    /// something the attribute cannot express, like the resource in the path.
    #[get("/team/roster")]
    pub async fn roster(ctx: RequestContext) -> ShieldResult<Response> {
        ctx.require_role("Manager")?;

        Ok(Response::json(
            HttpStatus::Ok,
            &json!({
                "access": "granted",
                "by": "ctx.require_role",
                "members": ["ada", "mike", "olga"],
            }),
        ))
    }

    /// A capability token. `ViewAuditLog` is declared in `security.rs` as
    /// `[Admin, Manager, Auditor]`, so those three get in and the other two do
    /// not.
    ///
    /// There is no role string on this route at all. Renaming the `Auditor` role
    /// to `Compliance` means editing one line of `security.rs`; this endpoint
    /// does not change.
    #[get("/logs")]
    #[cap(ViewAuditLog)]
    pub async fn audit_logs(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({
                "access": "granted",
                "by": "cap(ViewAuditLog)",
                "role": ctx.get_user_role(),
                "entries": 3,
            }),
        )
    }

    /// Two capabilities, OR-ed. `ManageBilling` is `[Admin, Manager]` and
    /// `RefundOrder` is `[Operator]`, so any of the three gets through while
    /// `Auditor` and `Viewer` do not.
    ///
    /// This is the case that motivates capabilities: refunds belong to
    /// operators, billing configuration belongs to managers, and the endpoint
    /// that accepts a refund should not have to care which of the two the caller
    /// came in on.
    #[post("/refunds")]
    #[cap(ManageBilling, RefundOrder)]
    pub async fn refund(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Created,
            &json!({
                "access": "granted",
                "by": "cap(ManageBilling, RefundOrder)",
                "role": ctx.get_user_role(),
                "refund_id": "rf_1024",
            }),
        )
    }

    /// `DeleteAccount` is `[Admin]`. Note that `Admin` is *also* the framework's
    /// super-user role: `has_role` grants it everything, including roles this
    /// example never mentions.
    #[delete("/accounts/:id")]
    #[cap(DeleteAccount)]
    pub async fn delete_account(ctx: RequestContext) -> Response {
        let id = ctx.param("id").unwrap_or("?").to_string();
        Response::ok(format!("deleted account {id}"))
    }
}
