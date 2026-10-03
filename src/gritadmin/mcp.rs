//! The `/admin/mcp` capability manager.
//!
//! Exposes what the agent surface actually contains: which tools exist, who may
//! call them, whether their kill switch is on, and how they have been behaving.
//! The page is read-mostly on purpose — the only mutation available is the
//! per-tool kill switch, because that is the one control an operator needs while
//! an agent is misbehaving.

use maud::{html, Markup};
use serde_json::json;

use crate::gritadmin::shell::admin_shell;
use crate::http::response::{HttpStatus, Response};
use crate::mcp::audit::audit;
use crate::mcp::protocol::McpError;
use crate::mcp::registry::registry;
use crate::routing::engine::RequestContext;

/// Where a capability manager toggle posts to.
pub const TOGGLE_PATH: &str = "/admin/mcp/api/toggle";

/// Read a single form field as a plain string.
fn field(ctx: &RequestContext, key: &str) -> Option<String> {
    ctx.form
        .fields
        .get(key)
        .and_then(|values| values.first())
        .map(|value| value.to_string())
}

/// Interpret a submitted `enabled` value.
///
/// Case-insensitive on purpose: an operator who posts `FALSE` and gets an
/// *enabled* tool is the dangerous failure mode here, not the reverse.
fn parse_enabled(value: &str) -> bool {
    matches!(value.trim().to_ascii_lowercase().as_str(), "true" | "1" | "on")
}

fn outcome_colour(outcome: &str) -> &'static str {
    match outcome {
        "ok" => "text-emerald-400",
        "denied" | "rejected" => "text-amber-400",
        "invalid" => "text-sky-400",
        _ => "text-rose-400",
    }
}

fn stat_card(label: &str, value: String, tone: &'static str) -> Markup {
    html! {
        div class="bg-gray-950 border border-gray-800 rounded-xl p-4 shadow-xl" {
            p class="text-gray-500 text-xs uppercase tracking-wider" { (label) }
            p class=(format!("text-2xl font-mono mt-1 {}", tone)) { (value) }
        }
    }
}

/// The JSON snapshot behind the page. Kept separate so monitoring and scripted
/// checks do not have to scrape HTML.
pub async fn admin_mcp_api_handler(_ctx: RequestContext) -> Response {
    let reg = registry();
    let rows = reg.admin_rows();
    let stats = audit().stats();

    Response::json(
        HttpStatus::Ok,
        &json!({
            "tools": rows,
            "resources": reg.resources().iter().map(|r| json!({
                "uri": r.uri,
                "name": r.name,
                "description": r.description,
                "mimeType": r.mime_type,
                "requiredRole": r.required_role,
            })).collect::<Vec<_>>(),
            "prompts": reg.prompts().iter().map(|p| json!({
                "name": p.name,
                "description": p.description,
                "arguments": p.declared_arguments(),
                "requiredRole": p.required_role,
            })).collect::<Vec<_>>(),
            "audit": {
                "total": stats.total,
                "failures": stats.failures,
                "denied": stats.denied,
                "averageDurationMicros": stats.average_duration_micros,
            }
        }),
    )
}

/// Flip one tool's kill switch.
pub async fn admin_mcp_toggle_handler(ctx: RequestContext) -> Response {
    let name = match field(&ctx, "name") {
        Some(name) if !name.trim().is_empty() => name.trim().to_string(),
        _ => {
            return Response::json(
                HttpStatus::BadRequest,
                &json!({ "error": "a 'name' field is required" }),
            )
        }
    };

    let enabled = field(&ctx, "enabled")
        .map(|value| parse_enabled(&value))
        .unwrap_or(false);

    // Only a tool the operator can actually see may be toggled: an unknown name
    // must not be distinguishable from a forbidden one.
    match registry().set_enabled(&name, enabled) {
        Ok(()) => Response::json(
            HttpStatus::Ok,
            &json!({ "tool": name, "enabled": enabled }),
        ),
        Err(McpError::NotFound(_)) => Response::json(
            HttpStatus::NotFound,
            &json!({ "error": format!("unknown MCP tool '{}'", name) }),
        ),
        Err(error) => Response::json(
            HttpStatus::InternalServerError,
            &json!({ "error": error.to_string() }),
        ),
    }
}

fn render_tools_table() -> Markup {
    let rows = registry().admin_rows();

    if rows.is_empty() {
        return html! {
            div class="border border-dashed border-gray-700 rounded-xl p-8 text-center text-gray-500" {
                p { "No MCP tools are registered." }
                p class="text-xs mt-2" {
                    "Annotate an implementation with #[mcp_tool] and it will appear here."
                }
            }
        };
    }

    html! {
        div class="overflow-x-auto" {
            table class="w-full text-left text-xs" {
                thead {
                    tr class="text-gray-500 uppercase tracking-wider" {
                        th class="p-2" { "Tool" }
                        th class="p-2" { "Service" }
                        th class="p-2" { "Required role" }
                        th class="p-2" { "Kill switch" }
                        th class="p-2 text-right" { "Calls" }
                        th class="p-2 text-right" { "Errors" }
                        th class="p-2 text-right" { "Denied" }
                        th class="p-2" { "Last call" }
                    }
                }
                tbody {
                    @for row in rows {
                        tr class="border-t border-gray-800 hover:bg-gray-900/40" {
                            td class="p-2" {
                                div class="font-mono text-emerald-400" { (row.name.clone()) }
                                @if !row.description.is_empty() {
                                    p class="text-gray-500 mt-1 max-w-md" { (row.description.clone()) }
                                }
                            }
                            td class="p-2 text-gray-400" { (row.service.clone()) }
                            td class="p-2" {
                                @match row.required_role {
                                    Some(role) => {
                                        span class="px-2 py-0.5 rounded bg-sky-950/50 text-sky-300 border border-sky-800/50 font-mono" {
                                            (role)
                                        }
                                    }
                                    None => {
                                        span class="text-gray-600 italic" { "any role" }
                                    }
                                }
                            }
                            td class="p-2" {
                                form
                                    hx-post=(TOGGLE_PATH)
                                    hx-target="#main-content"
                                    hx-swap="innerHTML"
                                    hx-indicator="body" {
                                    input type="hidden" name="name" value=(row.name.clone());
                                    input type="hidden" name="enabled" value=(
                                        if row.enabled { "false" } else { "true" }
                                    );
                                    button
                                        type="submit"
                                        class=(
                                            if row.enabled {
                                                "px-2 py-1 rounded border border-emerald-800/60 bg-emerald-950/40 text-emerald-400 font-mono"
                                            } else {
                                                "px-2 py-1 rounded border border-rose-800/60 bg-rose-950/40 text-rose-400 font-mono"
                                            }
                                        ) {
                                        @if row.enabled { "enabled" } @else { "disabled" }
                                    }
                                }
                                @if let Some(error) = &row.counters.last_error {
                                    p class="text-rose-400 mt-1 max-w-xs" { (error.clone()) }
                                }
                            }
                            td class="p-2 text-right font-mono" { (row.counters.invocations.to_string()) }
                            td class="p-2 text-right font-mono text-rose-400" { (row.counters.failures.to_string()) }
                            td class="p-2 text-right font-mono text-amber-400" { (row.counters.denied.to_string()) }
                            td class="p-2 text-gray-500 font-mono" {
                                (row.counters.last_invoked_at.clone().unwrap_or_else(|| "never".to_string()))
                            }
                        }
                    }
                }
            }
        }
    }
}

fn render_resources() -> Markup {
    let resources = registry().resources();

    html! {
        div class="overflow-x-auto" {
            table class="w-full text-left text-xs" {
                thead {
                    tr class="text-gray-500 uppercase tracking-wider" {
                        th class="p-2" { "URI" }
                        th class="p-2" { "Name" }
                        th class="p-2" { "Type" }
                        th class="p-2" { "Required role" }
                    }
                }
                tbody {
                    @for resource in resources {
                        tr class="border-t border-gray-800" {
                            td class="p-2 font-mono text-sky-300" { (resource.uri) }
                            td class="p-2 text-gray-300" { (resource.name) }
                            td class="p-2 text-gray-500 font-mono" { (resource.mime_type) }
                            td class="p-2" {
                                @match resource.required_role {
                                    Some(role) => { span class="text-sky-300 font-mono" { (role) } }
                                    None => { span class="text-gray-600 italic" { "any role" } }
                                }
                            }
                        }
                    }
                }
            }
            @if resources.is_empty() {
                p class="text-gray-500 text-xs p-2" { "No MCP resources are registered." }
            }
        }
    }
}

fn render_prompts() -> Markup {
    let prompts = registry().prompts();

    html! {
        div class="overflow-x-auto" {
            table class="w-full text-left text-xs" {
                thead {
                    tr class="text-gray-500 uppercase tracking-wider" {
                        th class="p-2" { "Prompt" }
                        th class="p-2" { "Arguments" }
                        th class="p-2" { "Required role" }
                    }
                }
                tbody {
                    @for prompt in prompts {
                        tr class="border-t border-gray-800" {
                            td class="p-2" {
                                div class="font-mono text-violet-300" { (prompt.name) }
                                @if !prompt.description.is_empty() {
                                    p class="text-gray-500 mt-1 max-w-md" { (prompt.description) }
                                }
                            }
                            td class="p-2" {
                                @let arguments = prompt.declared_arguments();
                                @if arguments.is_empty() {
                                    span class="text-gray-600 italic" { "none" }
                                } @else {
                                    @for argument in arguments {
                                        span
                                            class="inline-block mr-1 mb-1 px-2 py-0.5 rounded bg-gray-900 border border-gray-700 font-mono"
                                            title=(
                                                argument.description.clone()
                                                    .unwrap_or_else(|| "no description".to_string())
                                            ) {
                                            (argument.name.clone())
                                            @if argument.required { "!" } @else { "?" }
                                        }
                                    }
                                }
                            }
                            td class="p-2" {
                                @match prompt.required_role {
                                    Some(role) => { span class="text-sky-300 font-mono" { (role) } }
                                    None => { span class="text-gray-600 italic" { "any role" } }
                                }
                            }
                        }
                    }
                }
            }
            @if prompts.is_empty() {
                p class="text-gray-500 text-xs p-2" { "No MCP prompts are registered." }
            }
        }
    }
}

fn render_audit_tail() -> Markup {
    let entries = audit().recent(15);

    html! {
        div class="overflow-x-auto" {
            table class="w-full text-left text-xs" {
                thead {
                    tr class="text-gray-500 uppercase tracking-wider" {
                        th class="p-2" { "When" }
                        th class="p-2" { "Method" }
                        th class="p-2" { "Target" }
                        th class="p-2" { "Caller" }
                        th class="p-2" { "Outcome" }
                        th class="p-2 text-right" { "µs" }
                    }
                }
                tbody {
                    @for entry in &entries {
                        tr class="border-t border-gray-800" {
                            td class="p-2 text-gray-500 font-mono" { (entry.timestamp.clone()) }
                            td class="p-2 text-gray-300 font-mono" { (entry.method.clone()) }
                            td class="p-2 font-mono text-sky-300" { (entry.target.clone()) }
                            td class="p-2 text-gray-400" { (entry.caller.clone()) }
                            td class=(format!("p-2 font-mono {}", outcome_colour(&entry.outcome))) {
                                (entry.outcome.clone())
                                @if let Some(detail) = &entry.detail {
                                    p class="text-gray-500 mt-1 max-w-sm" { (detail.clone()) }
                                }
                            }
                            td class="p-2 text-right font-mono text-gray-500" {
                                (entry.duration_micros.to_string())
                            }
                        }
                    }
                }
            }
            @if entries.is_empty() {
                p class="text-gray-500 text-xs p-2" { "No MCP calls have been recorded yet." }
            }
        }
    }
}

/// The capability manager page.
pub async fn admin_mcp_page_handler(ctx: RequestContext) -> Response {
    let reg = registry();
    let stats = audit().stats();
    let is_htmx = ctx
        .headers
        .get("hx-request")
        .map(|values| values.iter().any(|value| value.eq_ignore_ascii_case("true")))
        .unwrap_or(false);

    let enabled_count = reg.admin_rows().iter().filter(|row| row.enabled).count();

    let content = html! {
        div class="space-y-6" {
            header {
                h2 class="text-2xl font-bold text-gray-100" { "MCP Capability Manager" }
                p class="text-gray-500 text-sm mt-1" {
                    "What an AI agent can see and do on this service."
                }
            }

            div class="grid grid-cols-2 md:grid-cols-4 gap-4" {
                (stat_card(
                    "Tools enabled",
                    format!("{} / {}", enabled_count, reg.tool_count()),
                    "text-emerald-400",
                ))
                (stat_card("Resources", reg.resource_count().to_string(), "text-sky-400"))
                (stat_card("Prompts", reg.prompt_count().to_string(), "text-violet-400"))
                (stat_card(
                    "MCP calls audited",
                    stats.total.to_string(),
                    if stats.denied > 0 { "text-amber-400" } else { "text-gray-200" },
                ))
            }

            section class="bg-gray-950 border border-gray-800 rounded-xl p-4 shadow-xl" {
                h3 class="text-lg font-bold text-emerald-400 mb-3" { "Tools" }
                (render_tools_table())
            }

            section class="bg-gray-950 border border-gray-800 rounded-xl p-4 shadow-xl" {
                h3 class="text-lg font-bold text-sky-400 mb-3" { "Resources" }
                (render_resources())
            }

            section class="bg-gray-950 border border-gray-800 rounded-xl p-4 shadow-xl" {
                h3 class="text-lg font-bold text-violet-400 mb-3" { "Prompts" }
                (render_prompts())
            }

            section class="bg-gray-950 border border-gray-800 rounded-xl p-4 shadow-xl" {
                h3 class="text-lg font-bold text-gray-300 mb-3" { "Recent agent activity" }
                (render_audit_tail())
            }
        }
    };

    admin_shell("MCP Capability Manager", content, is_htmx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::xss::UntrustedString;

    #[test]
    fn outcome_colours_distinguish_failures_from_success() {
        assert_eq!(outcome_colour("ok"), "text-emerald-400");
        assert_eq!(outcome_colour("denied"), "text-amber-400");
        assert_eq!(outcome_colour("rejected"), "text-amber-400");
        assert_eq!(outcome_colour("invalid"), "text-sky-400");
        assert_eq!(outcome_colour("failed"), "text-rose-400");
    }

    /// An unknown outcome must not render as a success.
    #[test]
    fn an_unknown_outcome_is_never_styled_as_success() {
        assert_ne!(outcome_colour("OK"), outcome_colour("ok"));
        assert_ne!(outcome_colour(""), "text-emerald-400");
    }

    fn ctx_with_form(fields: &[(&str, &str)]) -> RequestContext {
        let mut ctx = RequestContext::new();
        for (key, value) in fields {
            ctx.form
                .fields
                .insert((*key).to_string(), vec![UntrustedString::new((*value).to_string())]);
        }
        ctx
    }

    #[test]
    fn a_missing_form_field_reads_as_absent() {
        let ctx = ctx_with_form(&[("enabled", "true")]);
        assert_eq!(field(&ctx, "name"), None);
        assert_eq!(field(&ctx, "enabled").as_deref(), Some("true"));
    }

    /// The toggle endpoint accepts the spellings a hand-written curl or an
    /// htmx form will actually send — and must never read `FALSE` as "on".
    #[test]
    fn enabled_values_accept_the_usual_truthy_spellings() {
        for truthy in ["true", "1", "on", "TRUE", " on ", "True"] {
            assert!(parse_enabled(truthy), "'{}' should enable", truthy);
        }

        for falsy in ["false", "0", "off", "", "no", "FALSE", "Off"] {
            assert!(!parse_enabled(falsy), "'{}' should disable", falsy);
        }
    }

    #[test]
    fn a_missing_enabled_field_leaves_the_tool_disabled() {
        // Defaulting to `false` keeps an incomplete submission from widening
        // access to a capability.
        assert!(!field(&ctx_with_form(&[]), "enabled").map(|v| parse_enabled(&v)).unwrap_or(false));
    }

    /// The page must render even with nothing registered, since an operator
    /// needs to see that the agent surface is empty rather than a 500.
    #[test]
    fn the_page_renders_with_no_capabilities_registered() {
        let ctx = ctx_with_form(&[]);
        let html = render_tools_table().into_string();
        assert!(html.contains("No MCP tools are registered"));
        drop(ctx);
    }

    #[test]
    fn the_audit_table_renders_an_empty_state() {
        audit().clear();
        let html = render_audit_tail().into_string();
        assert!(html.contains("No MCP calls have been recorded"));
    }
}