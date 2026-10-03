//! Prompts — reusable, parameterised instructions.
//!
//! A prompt is a user-controlled template: the *user* picks it from a list and
//! fills in the arguments, so the result lands in their message box rather than
//! being executed. That makes prompts the right place for "how we do X here"
//! knowledge and the wrong place for anything that changes state.

use gritshield::mcp::prompt::McpPromptMessage;
use gritshield::mcp_prompt;
use serde::Deserialize;
use serde_json::Value;

// ---------------------------------------------------------------------------
// Argument syntax
// ---------------------------------------------------------------------------
//
// rustc parses attribute arguments as meta items, where a bare list entry like
// `summary("...")` is rejected unless it is the only entry. So declarations
// travel as two delimited strings:
//
//     arguments   = "summary!,severity?"
//     descriptions = "summary=One-line summary;severity=low | medium | high"
//
//   `!`  required
//   `?`  optional
//   bare optional
//
// Required arguments are enforced centrally by the engine *before* the handler
// runs, so a prompt cannot forget to check its own contract. The `arguments`
// declaration is what `prompts/list` advertises to the client — if you declare
// an argument the model does not send, you get a protocol error, not a panic.
//
// The handler itself takes a *single* parameter: the whole argument payload.
// It can be `serde_json::Value` or any `Deserialize` struct.

// ---------------------------------------------------------------------------
// Typed payload
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SummarizeArgs {
    pub summary: String,
    #[serde(default)]
    pub severity: Option<String>,
}

#[mcp_prompt(
    name = "summarize_incident",
    description = "Write a stakeholder-facing summary of an incident",
    arguments = "summary!,severity?",
    descriptions = "summary=One-line description of what happened;\
                    severity=One of low, medium, high (default: medium)"
)]
pub async fn summarize_incident(args: SummarizeArgs) -> Result<Vec<McpPromptMessage>, String> {
    let severity = args.severity.unwrap_or_else(|| "medium".to_string());

Ok(vec![McpPromptMessage::user(format!(
        "Summarize this {} severity incident for a non-technical stakeholder.\n\n\
         Include: impact, who was affected, and what we did about it.\n\n\
         Incident: {}",
        severity, args.summary
    ))])
}

// ---------------------------------------------------------------------------
// Raw payload
// ---------------------------------------------------------------------------

#[mcp_prompt(
    name = "onboarding_tour",
    description = "A guided tour of what this MCP server can do",
    arguments = "focus?",
    descriptions = "focus=Optional subsystem to focus on"
)]
pub async fn onboarding_tour(args: Value) -> Result<Vec<McpPromptMessage>, String> {
    let focus = args
        .get("focus")
        .and_then(Value::as_str)
        .unwrap_or("everything");

Ok(vec![McpPromptMessage::user(format!(
        "You are connected to a GritShield MCP server. Focus area: {}.\n\n\
         Before you start:\n\
         1. Call `resources/read` on `gritshield://system/runbook`.\n\
         2. Call `tools/list` and tell the user what is available.\n\
         3. Ask which incident they want to work on before calling any tool.\n\n\
         Tools that delete or mutate state stay disabled until an operator \
         explicitly enables them.",
        focus
    ))])
}

// ---------------------------------------------------------------------------
// Role-gated prompt
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct AuditArgs {
    pub table: String,
    pub range: String,
}

#[mcp_prompt(
    name = "audit_query",
    description = "Draft a read-only SQL audit query",
    required_role = "Admin",
    arguments = "table!,range",
    descriptions = "table=Table to audit;range=Time range, e.g. last 7 days"
)]
pub async fn audit_query(args: AuditArgs) -> Result<Vec<McpPromptMessage>, String> {
Ok(vec![McpPromptMessage::user(format!(
        "Draft a read-only SQL query auditing changes to `{}` over {}.\n\
         Do not include INSERT, UPDATE or DELETE.",
        args.table, args.range
    ))])
}