//! Tools — the things an agent can *do*.
//!
//! # How `#[mcp_tool]` works
//!
//! The attribute rewrites your function into two things:
//!
//! 1. your function, unchanged, and
//! 2. an `inventory` registration holding a pointer to it.
//!
//! Because registration happens through `inventory::submit!`, the handler is
//! found at runtime without any build step, and — importantly — the signature
//! is checked *at compile time*: the first parameter must be a
//! `RequestContext`, the second is the argument payload, and the return type
//! must be a `Result`. A mis-ordered signature fails the build rather than
//! misbehaving in production.

use gritshield::routing::engine::RequestContext;
use gritshield::mcp_tool;
use serde::Deserialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// 0. An ungated, read-only tool
// ---------------------------------------------------------------------------
//
// With no `required_role`, anyone who can reach the endpoint may call it — and
// it shows up in `tools/list` even for an anonymous caller, while the
// role-gated tools below stay hidden. Use this shape for genuinely public
// information (a version string, a status page) and put `required_role` on
// everything that touches data or changes state.

#[mcp_tool(
    name = "server_status",
    service = "Platform",
    description = "Version and health of this MCP server"
)]
pub async fn server_status(_ctx: &RequestContext, _args: Value) -> Result<Value, String> {
    Ok(serde_json::json!({
        "framework": "GritShield",
        "version": env!("CARGO_PKG_VERSION"),
        "healthy": true,
    }))
}

// ---------------------------------------------------------------------------
// 1. Typed arguments — the everyday case
// ---------------------------------------------------------------------------

/// The payload is deserialized from JSON *before* your code runs, so a bad
/// request from the model is rejected by the schema check rather than by a
/// panic three frames deeper.
#[derive(Debug, Deserialize)]
pub struct SearchArgs {
    pub query: String,
    #[serde(default)]
    pub limit: u32,
}

#[mcp_tool(
    name = "search_incidents",
    service = "IncidentService",
    description = "Full-text search over incident records",
    schema = r#"{
        "type": "object",
        "properties": {
            "query":   { "type": "string",  "description": "Free-text query" },
            "limit":   { "type": "integer", "description": "Max results", "minimum": 1, "maximum": 100 }
        },
        "required": ["query"]
    }"#,
    required_role = "Operator"
)]
pub async fn search_incidents(
    ctx: &RequestContext,
    args: SearchArgs,
) -> Result<Value, String> {
    let limit = args.limit.clamp(1, 100);

    Ok(serde_json::json!({
        "query": args.query,
        "limit": limit,
        "hits": [],
        // The RequestContext is threaded through the transport, so your handler
        // always knows *who* is calling without any extra plumbing.
        "called_by": ctx.claims.as_ref().map(|c| c.sub.clone()),
    }))
}

// ---------------------------------------------------------------------------
// 2. Raw JSON arguments — for pass-through or dynamic tools
// ---------------------------------------------------------------------------

/// Taking `serde_json::Value` skips deserialization entirely. Useful when the
/// shape is genuinely dynamic, or when you want to forward the payload as-is.
#[mcp_tool(
    name = "run_query",
    service = "IncidentService",
    description = "Execute a read-only query and return the rows",
    schema = r#"{
        "type": "object",
        "properties": {
            "sql": { "type": "string", "description": "SQL to execute" }
        },
        "required": ["sql"]
    }"#,
    required_role = "Admin"
)]
pub async fn run_query(ctx: &RequestContext, args: Value) -> Result<Value, String> {
    let sql = args
        .get("sql")
        .and_then(Value::as_str)
        .ok_or_else(|| "a 'sql' string is required".to_string())?;

    // The role check above already ran, but defence in depth is cheap: the
    // guard is in the framework, this is the business rule.
    if !ctx
        .claims
        .as_ref()
        .map(|c| c.role == "Admin")
        .unwrap_or(false)
    {
        return Err("run_query requires the Admin role".to_string());
    }

    Ok(serde_json::json!({ "sql": sql, "rows": [] }))
}

// ---------------------------------------------------------------------------
// 3. A tool that ships disabled
// ---------------------------------------------------------------------------
//
// `enabled = false` starts with the kill switch off. Every call is refused
// before your handler runs, and the tool is hidden from `tools/list` so the
// model cannot even see it — it stays visible in `/admin/mcp`, where an
// operator can enable it.
//
//     → {"code":-32001,"message":"Tool 'delete_incident' is currently disabled
//                               by an administrator"}

#[mcp_tool(
    name = "delete_incident",
    service = "IncidentService",
    description = "Permanently delete an incident (destructive)",
    enabled = false,
    schema = r#"{
        "type": "object",
        "properties": {
            "id": { "type": "string" }
        },
        "required": ["id"]
    }"#,
    required_role = "Admin"
)]
pub async fn delete_incident(_ctx: &RequestContext, args: Value) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "an 'id' string is required".to_string())?;

    Ok(serde_json::json!({ "deleted": id }))
}

// ---------------------------------------------------------------------------
// 4. Failing gracefully
// ---------------------------------------------------------------------------

/// Returning `Err` does *not* fail the JSON-RPC call. The tool result is
/// returned with `isError: true`, which is the MCP convention: the model gets a
/// chance to read the message and correct itself. Use a protocol error
/// (invalid params, not found) only when retrying cannot possibly help.
#[mcp_tool(
    name = "acknowledge_incident",
    service = "IncidentService",
    description = "Acknowledge an incident on behalf of the on-call engineer",
    schema = r#"{
        "type": "object",
        "properties": {
            "id":     { "type": "string" },
            "note":   { "type": "string" }
        },
        "required": ["id"]
    }"#,
    required_role = "Operator"
)]
pub async fn acknowledge_incident(
    _ctx: &RequestContext,
    args: Value,
) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "an 'id' string is required".to_string())?;

    if id == "unknown" {
        return Err(format!("no incident with id '{}'", id));
    }

    Ok(serde_json::json!({ "acknowledged": id }))
}