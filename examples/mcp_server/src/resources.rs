//! Resources — read-only context an agent can pull in.
//!
//! The distinction from a tool matters for model behaviour: a tool is an
//! *action* the model chooses to take, a resource is *context* it reads. If you
//! are exposing data, prefer a resource; if you are changing state, use a tool.

use gritshield::mcp_resource;
use gritshield::routing::engine::RequestContext;

// ---------------------------------------------------------------------------
// A resource that takes the request context
// ---------------------------------------------------------------------------

#[mcp_resource(
    uri = "gritshield://incidents/recent",
    name = "recent_incidents",
    description = "The 20 most recently opened incidents",
    mime_type = "application/json",
    required_role = "Operator"
)]
pub async fn recent_incidents(_ctx: &RequestContext) -> Result<String, String> {
    Ok(serde_json::json!([
        { "id": "INC-1041", "title": "Elevated 5xx on /checkout", "severity": "high" },
        { "id": "INC-1042", "title": "Replication lag",            "severity": "medium" }
    ])
    .to_string())
}

// ---------------------------------------------------------------------------
// A resource with no context — a constant or environment-derived value
// ---------------------------------------------------------------------------

#[mcp_resource(
    uri = "gritshield://system/runbook",
    name = "incident_runbook",
    description = "How to handle a production incident here",
    mime_type = "text/markdown"
)]
pub async fn runbook() -> Result<String, String> {
    Ok(
        "# Incident runbook\n\n\
         1. Acknowledge the alert in the admin panel.\n\
         2. Check `/admin/metrics` for the failing subsystem.\n\
         3. Roll back the last deploy if it started within the hour.\n"
            .to_string(),
    )
}

// ---------------------------------------------------------------------------
// Taking the context by value also works
// ---------------------------------------------------------------------------
//
// `#[mcp_resource]` accepts either `ctx: RequestContext` or
// `ctx: &RequestContext` (or no context at all). Pick whichever your handler
// finds natural; the macro emits only the matching call.

#[mcp_resource(
    uri = "gritshield://system/whoami",
    name = "caller_identity",
    description = "Who the agent is authenticated as, and with what role",
    mime_type = "application/json"
)]
pub async fn whoami(ctx: RequestContext) -> Result<String, String> {
    Ok(serde_json::json!({
        "authenticated": ctx.claims.is_some(),
        "subject": ctx.claims.as_ref().map(|c| c.sub.clone()),
        "role": ctx.claims.as_ref().map(|c| c.role.clone()),
    })
    .to_string())
}