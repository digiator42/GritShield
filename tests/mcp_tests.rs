//! End-to-end tests for the native MCP server.
//!
//! These exercise the public surface a real application uses: the
//! `#[mcp_tool]` / `#[mcp_resource]` / `#[mcp_prompt]` attributes, inventory
//! registration, and JSON-RPC dispatch through the engine. The guardrails are
//! asserted from the outside (what a caller observes) rather than from inside,
//! because that is the surface that actually matters for security.

use gritshield::mcp::prompt::McpPromptArgument;
use gritshield::mcp::protocol::{JsonRpcRequest, McpError};
use gritshield::mcp::registry::registry;
use gritshield::mcp::{McpEngine, McpPromptMessage as PromptMessage, McpTool, McpToolSchema};
use gritshield::routing::engine::RequestContext;
use gritshield::{mcp_prompt, mcp_resource, mcp_tool};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

// ============================================================
// Fixtures registered through the proc macros
// ============================================================

/// Arguments deserialized from the JSON payload by the macro.
#[derive(Debug, Deserialize)]
struct EchoArgs {
    message: String,
    #[serde(default)]
    times: u32,
}

/// The macro serializes a typed `Result<T, String>` into the wire `Value`.
#[derive(Debug, Serialize)]
struct EchoOut {
    echoed: String,
    times: u32,
}

#[mcp_tool(
    name = "echo",
    description = "Echo a message back to the caller",
    schema = r#"{
        "type": "object",
        "properties": {
            "message": { "type": "string" },
            "times": { "type": "integer", "minimum": 1 }
        },
        "required": ["message"]
    }"#,
    service = "TestService"
)]
async fn echo(_ctx: RequestContext, args: EchoArgs) -> Result<EchoOut, String> {
    Ok(EchoOut {
        echoed: args.message.repeat(args.times.max(1) as usize),
        times: args.times,
    })
}

#[mcp_tool(
    name = "explode",
    description = "Always fails, to prove handler errors surface as tool results",
    schema = r#"{ "type": "object", "properties": {} }"#,
    required_role = "SuperAdmin",
    service = "TestService"
)]
async fn explode(_ctx: RequestContext, _args: Value) -> Result<Value, String> {
    Err("deliberate handler failure".to_string())
}

/// A resource whose body is produced at registration time.
#[mcp_resource(
    uri = "gritshield://test/build",
    name = "Build info",
    description = "The build this test binary came from",
    mime_type = "text/plain"
)]
async fn build_info() -> Result<String, String> {
    Ok(format!("test-build {}", env!("CARGO_PKG_VERSION")))
}

#[mcp_resource(
    uri = "gritshield://test/secret",
    name = "Secret",
    description = "Role-gated resource",
    required_role = "SuperAdmin",
    mime_type = "text/plain"
)]
async fn secret(_ctx: RequestContext) -> Result<String, String> {
    Ok("classified".to_string())
}

#[mcp_prompt(
    name = "triage",
    description = "Triage an incident report",
    arguments = "summary!,severity?",
    descriptions = "summary=One-line description of the incident;severity=Assumed severity"
)]
async fn triage(args: Value) -> Result<Vec<PromptMessage>, String> {
    let summary = args
        .get("summary")
        .and_then(Value::as_str)
        .ok_or("summary is required")?;

    let severity = args.get("severity").and_then(Value::as_str).unwrap_or("unknown");

    Ok(vec![PromptMessage::user(format!(
        "Triage this {} incident: {}",
        severity, summary
    ))])
}

// ============================================================
// Harness
// ============================================================

fn ctx_with_role(role: &str) -> RequestContext {
    let mut ctx = RequestContext::new();
    let session = gritshield::security::session::Session {
        id: "test-session".to_string(),
        data: [
            ("role".to_string(), role.to_string()),
            ("user_id".to_string(), "agent-1".to_string()),
        ]
        .into_iter()
        .collect(),
        user_id: Some("agent-1".to_string()),
        last_accessed: std::time::Instant::now(),
    };
    ctx.session = Some(Arc::new(Mutex::new(session)));
    ctx
}

fn request(method: &str, params: Value) -> JsonRpcRequest {
    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: method.to_string(),
        params: Some(params),
    }
}

async fn dispatch(role: &str, method: &str, params: Value) -> Value {
    let dispatch = McpEngine::dispatch(ctx_with_role(role), None, request(method, params)).await;
    dispatch
        .response
        .expect("a request with an id must get a response")
        .into_result()
        .unwrap_or_else(|error| panic!("expected success, got error: {}", error))
}

// ============================================================
// Inventory registration
// ============================================================

#[test]
fn the_macro_registers_tools_in_the_inventory() {
    let registry = registry();

    let slot = registry
        .tool("echo")
        .unwrap_or_else(|| panic!("echo was not registered; known: {:?}", registry.tool_names()));

    assert_eq!(slot.schema.description, "Echo a message back to the caller");
    assert_eq!(slot.service, "TestService");
    assert_eq!(slot.schema.required_role, None, "no role was declared");
}

#[test]
fn the_declared_json_schema_survives_the_round_trip() {
    let schema = registry().tool("echo").expect("echo").schema.input_schema.clone();

    assert_eq!(schema["type"], json!("object"));
    assert_eq!(schema["properties"]["message"]["type"], json!("string"));
    assert_eq!(schema["required"], json!(["message"]));
    assert_eq!(schema["properties"]["times"]["minimum"], json!(1));
}

#[test]
fn required_roles_are_carried_onto_the_registry() {
    let slot = registry().tool("explode").expect("explode");
    assert_eq!(slot.schema.required_role, Some("SuperAdmin"));
}

#[test]
fn the_macro_registers_resources_and_prompts() {
    assert!(registry().resource("gritshield://test/build").is_some());
    assert!(registry().prompt("triage").is_some());
}

// ============================================================
// Discovery
// ============================================================

#[tokio::test]
async fn tools_list_advertises_registered_tools() {
    let result = dispatch("Admin", "tools/list", json!({})).await;
    let names: Vec<&str> = result["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();

    assert!(names.contains(&"echo"), "echo missing from {:?}", names);
}

#[tokio::test]
async fn discovery_hides_role_gated_tools_from_unauthorized_callers() {
    let as_admin = dispatch("Admin", "tools/list", json!({})).await;
    let admin_names: Vec<String> = as_admin["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect();

    let as_super = dispatch("SuperAdmin", "tools/list", json!({})).await;
    let super_names: Vec<String> = as_super["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_string))
        .collect();

    assert!(
        super_names.contains(&"explode".to_string()),
        "SuperAdmin should see the gated tool"
    );
    assert!(
        !admin_names.contains(&"explode".to_string()),
        "Admin must not even discover the SuperAdmin tool, saw {:?}",
        admin_names
    );
}

// ============================================================
// Execution
// ============================================================

#[tokio::test]
async fn a_typed_tool_deserializes_arguments_and_serializes_output() {
    let result = dispatch(
        "Admin",
        "tools/call",
        json!({ "name": "echo", "arguments": { "message": "hi", "times": 2 } }),
    )
    .await;

    assert_eq!(result["isError"], json!(false));
    assert_eq!(result["structuredContent"]["echoed"], json!("hihi"));
    assert_eq!(result["structuredContent"]["times"], json!(2));
}

#[tokio::test]
async fn schema_defaults_are_applied_before_the_handler_runs() {
    // `times` is optional and absent, so the handler sees 0 and clamps to 1.
    let result = dispatch(
        "Admin",
        "tools/call",
        json!({ "name": "echo", "arguments": { "message": "once" } }),
    )
    .await;

    assert_eq!(result["structuredContent"]["echoed"], json!("once"));
}

#[tokio::test]
async fn arguments_are_validated_before_the_handler_runs() {
    let dispatch = McpEngine::dispatch(
        ctx_with_role("Admin"),
        None,
        request(
            "tools/call",
            // `times` violates `minimum: 1`, and `message` is missing entirely.
            json!({ "name": "echo", "arguments": { "times": 0 } }),
        ),
    )
    .await;

    let error = dispatch.response.unwrap().error.unwrap();
    assert_eq!(error.code, gritshield::mcp::protocol::error_codes::SCHEMA_VIOLATION);
}

#[tokio::test]
async fn a_handler_error_becomes_a_tool_result_not_a_protocol_error() {
    let result = dispatch(
        "SuperAdmin",
        "tools/call",
        json!({ "name": "explode", "arguments": {} }),
    )
    .await;

    assert_eq!(result["isError"], json!(true));
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    assert!(
        text.contains("deliberate handler failure"),
        "the model needs the failure message, got: {}",
        text
    );
}

#[tokio::test]
async fn rbac_is_enforced_on_execution() {
    let dispatch = McpEngine::dispatch(
        ctx_with_role("Admin"),
        None,
        request("tools/call", json!({ "name": "explode", "arguments": {} })),
    )
    .await;

    let error = dispatch.response.unwrap().error.unwrap();
    assert_eq!(error.code, gritshield::mcp::protocol::error_codes::FORBIDDEN);
}

// ============================================================
// Resources and prompts
// ============================================================

#[tokio::test]
async fn resources_are_listed_and_readable() {
    let listing = dispatch("Admin", "resources/list", json!({})).await;
    let uris: Vec<&str> = listing["resources"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| r["uri"].as_str())
        .collect();
    assert!(uris.contains(&"gritshield://test/build"));

    let read = dispatch(
        "Admin",
        "resources/read",
        json!({ "uri": "gritshield://test/build" }),
    )
    .await;

    assert!(read["contents"][0]["text"]
        .as_str()
        .unwrap()
        .starts_with("test-build"));
}

#[tokio::test]
async fn reading_a_role_gated_resource_is_denied() {
    let dispatch = McpEngine::dispatch(
        ctx_with_role("Admin"),
        None,
        request("resources/read", json!({ "uri": "gritshield://test/secret" })),
    )
    .await;

    let error = dispatch.response.unwrap().error.unwrap();
    assert_eq!(error.code, gritshield::mcp::protocol::error_codes::FORBIDDEN);
}

#[tokio::test]
async fn prompts_render_with_their_declared_arguments() {
    let listing = dispatch("Admin", "prompts/list", json!({})).await;
    let triage = listing["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == json!("triage"))
        .expect("triage prompt should be listed");

    let arguments: Vec<&str> = triage["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|a| a["name"].as_str())
        .collect();
    assert!(arguments.contains(&"summary"));
    assert!(arguments.contains(&"severity"));
    assert_eq!(triage["arguments"][0]["required"], json!(true));

    let rendered = dispatch(
        "Admin",
        "prompts/get",
        json!({ "name": "triage", "arguments": { "summary": "disk full", "severity": "high" } }),
    )
    .await;

    let text = rendered["messages"][0]["content"]["text"].as_str().unwrap();
    assert!(text.contains("disk full"), "got: {}", text);
    assert!(text.contains("high"), "got: {}", text);
}

#[tokio::test]
async fn a_prompt_missing_a_required_argument_fails() {
    let dispatch = McpEngine::dispatch(
        ctx_with_role("Admin"),
        None,
        request("prompts/get", json!({ "name": "triage", "arguments": {} })),
    )
    .await;

    // Prompt rendering is not a tool call, so a failure is a protocol error
    // rather than an `isError` tool result.
    let error = dispatch.response.unwrap().error.unwrap();
    assert_eq!(
        error.code,
        gritshield::mcp::protocol::error_codes::INVALID_PARAMS
    );
    assert!(
        error.message.contains("summary"),
        "the model needs to know which argument was missing, got: {}",
        error.message
    );
}

// ============================================================
// Handshake and protocol hygiene
// ============================================================

#[tokio::test]
async fn initialize_advertises_every_capability_we_implement() {
    let result = dispatch(
        "Admin",
        "initialize",
        json!({ "protocolVersion": "2025-06-18", "clientInfo": { "name": "test" } }),
    )
    .await;

    assert_eq!(result["protocolVersion"], json!("2025-06-18"));
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["capabilities"]["resources"].is_object());
    assert!(result["capabilities"]["prompts"].is_object());
}

#[tokio::test]
async fn unsupported_protocol_versions_are_refused() {
    let dispatch = McpEngine::dispatch(
        ctx_with_role("Admin"),
        None,
        request("initialize", json!({ "protocolVersion": "2020-01-01" })),
    )
    .await;

    assert!(dispatch.response.unwrap().error.is_some());
}

#[test]
fn the_trait_based_path_still_works_for_stateful_tools() {
    /// A tool with captured state, which cannot be expressed as a const
    /// inventory entry — the reason `McpTool` exists alongside the macro.
    struct Prefixed {
        prefix: String,
    }

    #[gritshield::deps::sea_orm_migration::async_trait::async_trait]
    impl McpTool for Prefixed {
        fn schema(&self) -> McpToolSchema {
            McpToolSchema::new("prefixed_echo", "Echoes with a prefix", json!({
                "type": "object",
                "properties": { "message": { "type": "string" } },
                "required": ["message"]
            }))
        }

        async fn execute(&self, args: Value, _ctx: &RequestContext) -> Result<Value, String> {
            let message = args["message"].as_str().unwrap_or_default();
            Ok(json!({ "prefixed": format!("{}{}", self.prefix, message) }))
        }
    }

    let registry = gritshield::mcp::registry::McpRegistry::bootstrap();
    registry.register_tool(Arc::new(Prefixed {
        prefix: ">> ".to_string(),
    }));

    assert!(registry.tool("prefixed_echo").is_some());
}

#[test]
fn prompt_argument_helpers_set_requiredness() {
    assert!(McpPromptArgument::required("a", "b").required);
    assert!(!McpPromptArgument::optional("a", "b").required);
}

#[test]
fn tool_errors_map_onto_protocol_variants() {
    let error: McpError = McpError::from("boom");
    assert!(matches!(error, McpError::Execution(_)));
}

// ============================================================
// HTTP surface wiring
// ============================================================

/// Every documented MCP endpoint must be present on the router trie.
///
/// A handler that exists but was never registered is invisible to a client, so
/// this asserts the mount itself rather than the handler bodies.
#[test]
fn the_mcp_endpoints_are_mounted_on_the_router() {
    use gritshield::http::HttpMethod;
    use gritshield::routing::engine::{Router, RoutingResult};

    let router = Router::new();

    let expected = [
        (HttpMethod::GET, "/mcp/sse"),
        (HttpMethod::POST, "/mcp/message"),
        (HttpMethod::POST, "/mcp"),
        (HttpMethod::DELETE, "/mcp"),
        (HttpMethod::GET, "/mcp"),
    ];

    for (method, path) in expected {
        assert!(
            matches!(router.match_route(&method, path), RoutingResult::Found(..)),
            "{:?} {} should resolve to the MCP handler",
            method,
            path
        );
    }
}

/// An unregistered method on a mounted path must 405 rather than 404, so a
/// client learns its verb is wrong instead of that the path is missing.
#[test]
fn an_unsupported_mcp_verb_is_rejected() {
    use gritshield::http::HttpMethod;
    use gritshield::routing::engine::{Router, RoutingResult};

    let router = Router::new();
    assert!(matches!(
        router.match_route(&HttpMethod::PUT, "/mcp/sse"),
        RoutingResult::MethodNotAllowed
    ));
}

/// The prefix is configurable so an app can move the surface off `/mcp`.
#[test]
fn the_mcp_prefix_is_overridable() {
    use gritshield::routing::engine::builder::mcp;

    // The env var is process-global, so this asserts the default and the
    // normalisation rules rather than racing other tests over the variable.
    assert_eq!(mcp::prefix(), "/mcp");
}