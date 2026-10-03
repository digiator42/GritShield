use crate::mcp::audit::{self, outcome};
use crate::mcp::auth::{self, IdentitySource, McpAuthOutcome};
use crate::mcp::protocol::{
    methods, tool_failure, tool_success, JsonRpcError, JsonRpcRequest, JsonRpcResponse, McpError,
    PROTOCOL_VERSION, SERVER_NAME, SERVER_VERSION, SUPPORTED_PROTOCOL_VERSIONS,
};
use crate::mcp::registry::registry;
use crate::mcp::schema::validate;
use crate::mcp::session::McpSession;
use crate::routing::engine::RequestContext;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Instant;

/// The identity attached to a dispatch, resolved once and reused for auditing.
#[derive(Clone)]
pub struct McpCaller {
    pub subject: String,
    pub role: Option<String>,
    pub client_ip: String,
}

impl McpCaller {
    fn from_ctx(ctx: &RequestContext, outcome: &McpAuthOutcome) -> Self {
        let identity = outcome.identity();
        Self {
            subject: identity
                .map(|i| i.subject.clone())
                .unwrap_or_else(|| "rejected".to_string()),
            role: identity.and_then(|i| i.role.clone()),
            client_ip: ctx.resolve_client_ip(),
        }
    }
}

/// The outcome of dispatching one JSON-RPC message.
pub struct McpDispatch {
    /// `None` for notifications, which by spec get no response.
    pub response: Option<JsonRpcResponse>,
    /// The HTTP status the transport should use if it has to report failure
    /// out-of-band (the SSE transport answers `202` regardless).
    pub http_status: u16,
}

impl McpDispatch {
    fn silent() -> Self {
        Self {
            response: None,
            http_status: 202,
        }
    }

    fn ok(id: Value, result: Value) -> Self {
        Self {
            response: Some(JsonRpcResponse::success(id, result)),
            http_status: 200,
        }
    }

    fn err(id: Value, error: McpError) -> Self {
        Self {
            response: Some(JsonRpcResponse::failure(id, error.to_json_rpc())),
            http_status: 200,
        }
    }

    /// True when the message produced no JSON-RPC error.
    pub fn is_successful(&self) -> bool {
        self.response
            .as_ref()
            .is_some_and(|r| r.error.is_none())
    }
}

/// The JSON-RPC dispatch engine.
///
/// This is the single choke point through which every MCP request passes, which
/// is what makes the guard order enforceable: authentication, then kill switch,
/// then RBAC, then schema pre-flight, and only then application code. Keeping
/// that sequence in one function is the point — scattering it across handlers is
/// how guardrails get skipped.
pub struct McpEngine;

/// How long a bearer token stays the adopted request identity.
///
/// The token's own `exp` was already verified before adoption; this only bounds
/// how long the derived in-memory claims remain meaningful for RBAC.
const BEARTER_IDENTITY_TTL_SECONDS: u64 = 3600;

impl McpEngine {
    /// Adopt a verified MCP bearer token as the request's identity.
    ///
    /// MCP authenticates on its own rather than through the HTTP middleware, so
    /// a bearer token presented only here produces an identity that the ambient
    /// request state knows nothing about. Every per-capability RBAC check asks
    /// `ctx.has_role(..)`, which reads that ambient state — so without this the
    /// token would be verified, recorded in the audit log, and then unable to
    /// authorize anything.
    ///
    /// Claims already present always win. Overwriting them would let an MCP
    /// credential silently outrank an authenticated session, and only a request
    /// with no identity of its own may be promoted this way.
    fn adopt_bearer_identity(ctx: &mut RequestContext, auth: &McpAuthOutcome) {
        if ctx.claims.is_some() || ctx.get_user_id().is_some() {
            return;
        }

        let identity = match auth.identity() {
            Some(identity) if identity.source == IdentitySource::BearerToken => identity,
            _ => return,
        };

        // A bearer identity without a role carries no authority to adopt;
        // inventing one would be the opposite of fail-closed.
        let role = match &identity.role {
            Some(role) => role.clone(),
            None => return,
        };

        ctx.claims = Some(crate::security::jwt::Claims::new(
            identity.subject.clone(),
            role,
            BEARTER_IDENTITY_TTL_SECONDS,
        ));
    }

    /// Dispatch one parsed JSON-RPC message.
    ///
    /// `session` is `None` for stateless transports (stdio, and HTTP requests
    /// that arrive without a session id).
    pub async fn dispatch(
        mut ctx: RequestContext,
        session: Option<Arc<McpSession>>,
        request: JsonRpcRequest,
    ) -> McpDispatch {
        let started = Instant::now();
        let auth = auth::authenticate(&ctx);

        if let McpAuthOutcome::Rejected { status, reason } = &auth {
            // Rejections still go through the protocol so the model gets a
            // structured reason rather than a bare socket error.
            audit::audit().record(
                &request.method,
                &target_label(&request),
                "rejected",
                None,
                &ctx.resolve_client_ip(),
                session.as_ref().map(|s| s.id.clone()),
                json!({}),
                outcome::REJECTED,
                Some(reason.clone()),
                started.elapsed().as_micros() as u64,
            );

            let id = request.id.clone().unwrap_or(Value::Null);
            let mut response =
                McpDispatch::err(id, McpError::Unauthenticated(reason.clone()));
            response.http_status = *status;
            return response;
        }

        if let Some(session) = &session {
            session.record_request();
        }

        Self::adopt_bearer_identity(&mut ctx, &auth);

        let caller = McpCaller::from_ctx(&ctx, &auth);

        if let Err(error) = request.validate_envelope() {
            audit::audit().record(
                &request.method,
                &target_label(&request),
                &caller.subject,
                caller.role.clone(),
                &caller.client_ip,
                session.as_ref().map(|s| s.id.clone()),
                json!({}),
                outcome::INVALID,
                Some(error.to_string()),
                started.elapsed().as_micros() as u64,
            );

            let id = request.id.clone().unwrap_or(Value::Null);
            return McpDispatch::err(id, error);
        }

        let result = Self::route(&ctx, session.as_deref(), &caller, &request).await;

        // Notifications never produce a response, so a failed notification is
        // observable only through the audit trail.
        if request.is_notification() {
            if let Err(error) = &result {
                audit::audit().record(
                    &request.method,
                    &target_label(&request),
                    &caller.subject,
                    caller.role.clone(),
                    &caller.client_ip,
                    session.as_ref().map(|s| s.id.clone()),
                    json!({}),
                    outcome::FAILED,
                    Some(error.to_string()),
                    started.elapsed().as_micros() as u64,
                );
            }
            return McpDispatch::silent();
        }

        let id = request.id.clone().unwrap_or(Value::Null);

        match result {
            Ok(value) => McpDispatch::ok(id, value),
            Err(error) => McpDispatch::err(id, error),
        }
    }

    async fn route(
        ctx: &RequestContext,
        session: Option<&McpSession>,
        caller: &McpCaller,
        request: &JsonRpcRequest,
    ) -> Result<Value, McpError> {
        match request.method.as_str() {
            methods::INITIALIZE => Self::initialize(ctx, session, request),
            methods::INITIALIZED => Ok(json!({})),
            methods::PING => Ok(json!({})),
            methods::TOOLS_LIST => Ok(Self::list_tools(ctx)),
            methods::TOOLS_CALL => Self::call_tool(ctx, caller, request).await,
            methods::RESOURCES_LIST => Ok(Self::list_resources(ctx)),
            methods::RESOURCES_TEMPLATES_LIST => Ok(json!({ "resourceTemplates": [] })),
            methods::RESOURCES_READ => Self::read_resource(ctx, request).await,
            methods::PROMPTS_LIST => Ok(Self::list_prompts()),
            methods::PROMPTS_GET => Self::get_prompt(request).await,
            methods::LOGGING_SET_LEVEL => Ok(json!({})),
            unknown => Err(McpError::InvalidRequest(format!(
                "Unknown MCP method '{}'",
                unknown
            ))),
        }
    }

    /// The handshake: agree a protocol revision and advertise capabilities.
    fn initialize(
        ctx: &RequestContext,
        session: Option<&McpSession>,
        request: &JsonRpcRequest,
    ) -> Result<Value, McpError> {
        let requested = request
            .params
            .as_ref()
            .and_then(|p| p.get("protocolVersion"))
            .and_then(Value::as_str)
            .unwrap_or(PROTOCOL_VERSION);

        // No silent downgrade: a client asking for a revision we do not speak
        // is told so, rather than being handed a mismatched session.
        if !SUPPORTED_PROTOCOL_VERSIONS.contains(&requested) {
            return Err(McpError::InvalidRequest(format!(
                "Unsupported MCP protocol version '{}'; this server speaks {}",
                requested,
                SUPPORTED_PROTOCOL_VERSIONS.join(", ")
            )));
        }

        let client_info = request
            .params
            .as_ref()
            .and_then(|p| p.get("clientInfo"))
            .cloned();

        if let Some(session) = session {
            session.set_protocol_version(requested);
            if let Some(info) = client_info.clone() {
                session.set_client_info(info);
            }
            session.initialized.store(true, std::sync::atomic::Ordering::Relaxed);
        }

        Ok(json!({
            "protocolVersion": requested,
            "capabilities": {
                "tools": { "listChanged": true },
                "resources": { "subscribe": false, "listChanged": true },
                "prompts": { "listChanged": true },
                "logging": {}
            },
            "serverInfo": {
                "name": SERVER_NAME,
                "version": SERVER_VERSION,
                "framework": "GritShield"
            },
            "instructions": "Tools are RBAC-gated. Call tools/list to discover what this identity may use.",
            "_gritshield": {
                "callerRole": ctx.get_user_role(),
                "authenticated": ctx.get_user_id().is_some()
            }
        }))
    }

    /// Discovery, filtered to what this caller is actually permitted to use.
    fn list_tools(ctx: &RequestContext) -> Value {
        let registry = registry();
        let visible = registry.visible_tool_names(ctx);

        let mut tools = Vec::with_capacity(visible.len());
        for name in visible {
            if let Some(slot) = registry.tool(&name) {
                // The kill switch removes a tool from discovery entirely rather
                // than advertising something that will only fail.
                if slot.is_enabled() {
                    tools.push(slot.schema.to_descriptor());
                }
            }
        }

        json!({ "tools": tools })
    }

    /// Execution: kill switch, then RBAC, then schema, then application code.
    async fn call_tool(
        ctx: &RequestContext,
        caller: &McpCaller,
        request: &JsonRpcRequest,
    ) -> Result<Value, McpError> {
        let started = Instant::now();
        let registry = registry();

        let name = request
            .params
            .as_ref()
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::InvalidParams("tools/call requires a 'name'".to_string()))?
            .to_string();

        let raw_arguments = request
            .params
            .as_ref()
            .and_then(|p| p.get("arguments"))
            .cloned()
            .unwrap_or_else(|| json!({}));

        let finish = |label: &str, detail: Option<String>| {
            audit::audit().record(
                methods::TOOLS_CALL,
                &name,
                &caller.subject,
                caller.role.clone(),
                &caller.client_ip,
                None,
                truncate_args(&raw_arguments),
                label,
                detail,
                started.elapsed().as_micros() as u64,
            );
        };

        let slot = registry.tool(&name).ok_or_else(|| {
            finish(outcome::INVALID, Some(format!("Unknown tool '{}'", name)));
            McpError::NotFound(format!("Unknown MCP tool '{}'", name))
        })?;

        // 1. Kill switch.
        if !slot.is_enabled() {
            slot.record_denial();
            finish(
                outcome::DENIED,
                Some(format!("Tool '{}' is disabled by an administrator", name)),
            );
            return Err(McpError::Disabled(format!(
                "Tool '{}' is currently disabled by an administrator",
                name
            )));
        }

        // 2. RBAC.
        if let Some(role) = slot.schema.required_role {
            if !ctx.has_role(role) {
                slot.record_denial();
                finish(
                    outcome::DENIED,
                    Some(format!("Missing role '{}'", role)),
                );
                return Err(McpError::Forbidden(format!(
                    "Role '{}' is required to invoke '{}'",
                    role, name
                )));
            }
        }

        // 3. Schema pre-flight, including default substitution.
        let checked = validate(&slot.schema.input_schema, &raw_arguments);
        if !checked.is_valid() {
            finish(
                outcome::INVALID,
                Some(checked.describe_violations()),
            );
            return Err(McpError::SchemaViolation(format!(
                "Invalid arguments for '{}': {}",
                name,
                checked.describe_violations()
            )));
        }

        // 4. Application code.
        let outcome_result = slot.invoke(ctx.clone(), checked.value).await;

        match outcome_result {
            Ok(value) => {
                slot.record_success();
                finish(outcome::OK, None);
                Ok(tool_success(value))
            }
            Err(error) => {
                slot.record_failure(&error.to_string());
                finish(outcome::FAILED, Some(error.to_string()));

                // A tool that ran and failed is a *successful* JSON-RPC call
                // with `isError: true`. The model needs to see the message to
                // decide whether to retry with different arguments.
                Ok(tool_failure(&error.to_string()))
            }
        }
    }

    fn list_resources(ctx: &RequestContext) -> Value {
        let resources: Vec<Value> = registry()
            .resources()
            .iter()
            .filter(|resource| match resource.required_role {
                Some(role) => ctx.has_role(role),
                None => true,
            })
            .map(|resource| resource.to_descriptor())
            .collect();

        json!({ "resources": resources })
    }

    async fn read_resource(ctx: &RequestContext, request: &JsonRpcRequest) -> Result<Value, McpError> {
        let uri = request
            .params
            .as_ref()
            .and_then(|p| p.get("uri"))
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::InvalidParams("resources/read requires a 'uri'".to_string()))?;

        let resource = registry().resource(uri).ok_or_else(|| {
            McpError::NotFound(format!("Unknown MCP resource '{}'", uri))
        })?;

        if let Some(role) = resource.required_role {
            if !ctx.has_role(role) {
                return Err(McpError::Forbidden(format!(
                    "Role '{}' is required to read '{}'",
                    role, uri
                )));
            }
        }

        let contents = (resource.reader)(ctx.clone()).await?;

        Ok(json!({
            "contents": [{
                "uri": contents.uri,
                "mimeType": contents.mime_type,
                "text": contents.text,
            }]
        }))
    }

    fn list_prompts() -> Value {
        let prompts: Vec<Value> = registry()
            .prompts()
            .iter()
            .map(|prompt| prompt.to_descriptor())
            .collect();

        json!({ "prompts": prompts })
    }

    async fn get_prompt(request: &JsonRpcRequest) -> Result<Value, McpError> {
        let name = request
            .params
            .as_ref()
            .and_then(|p| p.get("name"))
            .and_then(Value::as_str)
            .ok_or_else(|| McpError::InvalidParams("prompts/get requires a 'name'".to_string()))?;

        let prompt = registry()
            .prompt(name)
            .ok_or_else(|| McpError::NotFound(format!("Unknown MCP prompt '{}'", name)))?;

        let arguments = request
            .params
            .as_ref()
            .and_then(|p| p.get("arguments"))
            .cloned()
            .unwrap_or_else(|| json!({}));

        // Pre-flight the declared arguments, mirroring the tool schema check:
        // the prompt already advertised which arguments are required, so the
        // model should not be able to skip them and reach application code.
        let missing = prompt.missing_required(&arguments);
        if !missing.is_empty() {
            return Err(McpError::InvalidParams(format!(
                "prompt '{}' is missing required argument(s): {}",
                name,
                missing.join(", ")
            )));
        }

        let messages = (prompt.builder)(arguments).await?;

        let mut result = Map::new();
        if !prompt.description.is_empty() {
            result.insert("description".to_string(), json!(prompt.description));
        }
        result.insert(
            "messages".to_string(),
            Value::Array(
                messages
                    .into_iter()
                    .map(|message| {
                        json!({ "role": message.role, "content": message.content })
                    })
                    .collect(),
            ),
        );

        Ok(Value::Object(result))
    }
}

/// A best-effort label for the audit trail when a request names no target.
fn target_label(request: &JsonRpcRequest) -> String {
    request
        .params
        .as_ref()
        .and_then(|p| p.get("name").or_else(|| p.get("uri")))
        .and_then(Value::as_str)
        .unwrap_or("-")
        .to_string()
}

/// Shrink an argument payload before it reaches the audit ring buffer.
fn truncate_args(value: &Value) -> Value {
    let text = value.to_string();
    if text.len() <= 1024 {
        return value.clone();
    }
    let head: String = text.chars().take(1024).collect();
    json!({ "_truncated": head })
}

/// Parse a raw JSON-RPC body, mapping malformed input onto a protocol error.
pub fn parse_message(body: &[u8]) -> Result<JsonRpcRequest, JsonRpcError> {
    if body.is_empty() {
        return Err(JsonRpcError::new(
            crate::mcp::protocol::error_codes::INVALID_REQUEST,
            "Empty request body",
        ));
    }

    serde_json::from_slice(body).map_err(|error| {
        JsonRpcError::new(
            crate::mcp::protocol::error_codes::PARSE_ERROR,
            format!("Malformed JSON-RPC payload: {}", error),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::protocol::error_codes;
    use crate::mcp::schema::McpToolSchema;
    use crate::mcp::tool::McpTool;
    use sea_orm_migration::async_trait::async_trait;
    use std::sync::Mutex;

    /// A tool with a real schema and a scripted body, so the guard ordering can
    /// be exercised end to end.
    struct Guarded {
        schema: McpToolSchema,
    }

    #[async_trait]
    impl McpTool for Guarded {
        fn schema(&self) -> McpToolSchema {
            self.schema.clone()
        }

        async fn execute(&self, args: Value, _ctx: &RequestContext) -> Result<Value, String> {
            Ok(json!({ "received": args }))
        }
    }

    fn guarded_schema(name: &str) -> McpToolSchema {
        McpToolSchema::new(
            name,
            "A tool behind a role fence",
            json!({
                "type": "object",
                "properties": {
                    "count": { "type": "integer", "minimum": 1 }
                },
                "required": ["count"]
            }),
        )
    }

    /// Register a throwaway tool under a name unique to the calling test.
    ///
    /// The process-wide registry is shared, and cargo runs tests in parallel,
    /// so reusing one tool name across tests would let them overwrite each
    /// other's slot — including its kill-switch state.
    fn unique_tool(test: &str) -> String {
        let name = format!("test_{}_{}", test, uuid::Uuid::new_v4().simple());
        registry().register_tool(Arc::new(Guarded {
            schema: guarded_schema(&name),
        }));
        name
    }

    // -----------------------------------------------------------------------
    // Bearer identity adoption
    //
    // Regression cover for the case where MCP authenticates a bearer token but
    // the ambient request state knows nothing about it, leaving every
    // role-gated capability unreachable.
    // -----------------------------------------------------------------------

    fn bearer_outcome(subject: &str, role: Option<&str>, source: IdentitySource) -> McpAuthOutcome {
        McpAuthOutcome::Allowed(auth::McpIdentity {
            subject: subject.to_string(),
            role: role.map(|r| r.to_string()),
            source,
        })
    }

    #[test]
    fn a_verified_bearer_token_becomes_the_request_identity() {
        let mut ctx = RequestContext::new();
        assert!(ctx.claims.is_none());

        McpEngine::adopt_bearer_identity(
            &mut ctx,
            &bearer_outcome("agent-42", Some("Operator"), IdentitySource::BearerToken),
        );

        let claims = ctx
            .claims
            .as_ref()
            .expect("the bearer identity should be adopted");
        assert_eq!(claims.sub, "agent-42");
        assert_eq!(claims.role, "Operator");
        // This is the whole point: RBAC consults the request, not the token.
        assert!(ctx.has_role("Operator"));
    }

    #[test]
    fn an_existing_identity_is_never_overwritten() {
        // Claims from the framework's own middleware.
        let mut with_claims = RequestContext::new();
        with_claims.claims = Some(crate::security::jwt::Claims::new(
            "session-user".to_string(),
            "Viewer".to_string(),
            600,
        ));

        // A role carried by the session rather than by claims.
        let with_session = ctx_as("Viewer");

        for (label, mut ctx, expected_role) in [
            // The role lives in claims…
            ("claims", with_claims, Some("Viewer".to_string())),
            // …and here it lives in the session instead.
            ("session", with_session, Some("Viewer".to_string())),
        ] {
            let before = ctx
                .claims
                .as_ref()
                .map(|c| c.role.clone())
                .or_else(|| ctx.get_user_role());

            McpEngine::adopt_bearer_identity(
                &mut ctx,
                &bearer_outcome("agent-42", Some("Admin"), IdentitySource::BearerToken),
            );

            let after = ctx
                .claims
                .as_ref()
                .map(|c| c.role.clone())
                .or_else(|| ctx.get_user_role());

            assert_eq!(before, expected_role, "{} role before adoption", label);
            assert_eq!(after, expected_role, "{} must keep its own identity", label);
            assert!(
                !ctx.has_role("Admin"),
                "a bearer token must not elevate an existing {} identity",
                label
            );
        }
    }

    #[test]
    fn a_bearer_token_without_a_role_grants_nothing() {
        let mut ctx = RequestContext::new();

        McpEngine::adopt_bearer_identity(
            &mut ctx,
            &bearer_outcome("agent-42", None, IdentitySource::BearerToken),
        );

        assert!(ctx.claims.is_none(), "a roleless identity must not be invented");
        assert!(!ctx.has_role("Admin"));
    }

    #[test]
    fn a_session_or_anonymous_identity_is_not_adopted_as_a_bearer_token() {
        for source in [IdentitySource::SessionCookie, IdentitySource::Anonymous] {
            let mut ctx = RequestContext::new();
            McpEngine::adopt_bearer_identity(
                &mut ctx,
                &bearer_outcome("someone", Some("Admin"), source.clone()),
            );
            assert!(ctx.claims.is_none(), "{:?} must not set claims", source);
        }
    }

    fn ctx_as(role: &str) -> RequestContext {
        let mut ctx = RequestContext::new();
        let session = crate::security::session::Session {
            id: uuid::Uuid::new_v4().to_string(),
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

    #[tokio::test]
    async fn initialize_reports_server_identity_and_capabilities() {
        let response = McpEngine::dispatch(
            RequestContext::new(),
            None,
            request("initialize", json!({ "protocolVersion": PROTOCOL_VERSION })),
        )
        .await;

        let result = response.response.unwrap().result.unwrap();
        assert_eq!(result["protocolVersion"], json!(PROTOCOL_VERSION));
        assert_eq!(result["serverInfo"]["name"], json!("gritshield"));
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn initialize_refuses_an_unsupported_protocol_revision() {
        let response = McpEngine::dispatch(
            RequestContext::new(),
            None,
            request("initialize", json!({ "protocolVersion": "1999-01-01" })),
        )
        .await;

        let error = response.response.unwrap().error.unwrap();
        assert_eq!(error.code, error_codes::INVALID_REQUEST);
        assert!(error.message.contains("1999-01-01"));
    }

    #[tokio::test]
    async fn unknown_methods_are_rejected() {
        let response = McpEngine::dispatch(
            RequestContext::new(),
            None,
            request("tools/teleport", json!({})),
        )
        .await;

        assert!(response.response.unwrap().error.is_some());
    }

    #[tokio::test]
    async fn notifications_get_no_response() {
        let mut notif = request("notifications/initialized", json!({}));
        notif.id = None;

        let dispatch = McpEngine::dispatch(RequestContext::new(), None, notif).await;
        assert!(dispatch.response.is_none());
    }

    #[tokio::test]
    async fn the_kill_switch_blocks_execution_and_hides_from_discovery() {
        let name = unique_tool("killswitch");
        registry().set_enabled(&name, false).unwrap();

        let ctx = ctx_as("Admin");
        let dispatch = McpEngine::dispatch(
            ctx.clone(),
            None,
            request("tools/call", json!({ "name": name, "arguments": { "count": 3 } })),
        )
        .await;

        let error = dispatch.response.unwrap().error.unwrap();
        assert_eq!(error.code, error_codes::TOOL_DISABLED);

        // And it must not appear in tools/list for anyone.
        let listing = McpEngine::dispatch(ctx, None, request("tools/list", json!({}))).await;
        let tools = listing.response.unwrap().result.unwrap();
        assert!(
            !tools["tools"].as_array().unwrap().iter().any(|t| t["name"] == name),
            "a disabled tool leaked into discovery"
        );
    }

    #[tokio::test]
    async fn schema_violations_are_caught_before_the_handler_runs() {
        let name = unique_tool("schema");

        let dispatch = McpEngine::dispatch(
            ctx_as("Admin"),
            None,
            request("tools/call", json!({ "name": name, "arguments": { "count": 0 } })),
        )
        .await;

        let error = dispatch.response.unwrap().error.unwrap();
        assert_eq!(error.code, error_codes::SCHEMA_VIOLATION);
        assert!(error.data.is_some(), "violations should be reported structurally");
    }

    #[tokio::test]
    async fn unknown_tools_report_not_found() {
        let dispatch = McpEngine::dispatch(
            ctx_as("Admin"),
            None,
            request("tools/call", json!({ "name": "no_such_tool" })),
        )
        .await;

        let error = dispatch.response.unwrap().error.unwrap();
        assert_eq!(error.code, error_codes::NOT_FOUND);
    }

    #[tokio::test]
    async fn a_successful_call_returns_content_and_structured_output() {
        let name = unique_tool("success");

        let dispatch = McpEngine::dispatch(
            ctx_as("Admin"),
            None,
            request("tools/call", json!({ "name": name, "arguments": { "count": 2 } })),
        )
        .await;

        let result = dispatch.response.unwrap().result.unwrap();
        assert_eq!(result["isError"], json!(false));
        assert_eq!(result["structuredContent"]["received"]["count"], json!(2));
    }

    #[tokio::test]
    async fn rbac_denies_a_role_gated_tool() {
        let name = unique_tool("rbac");

        // Re-register with a role fence, under this test's own tool name.
        let mut schema = guarded_schema(&name);
        schema.required_role = Some("SuperAdmin");
        registry().register_tool(Arc::new(Guarded { schema }));

        let dispatch = McpEngine::dispatch(
            ctx_as("Admin"),
            None,
            request("tools/call", json!({ "name": name, "arguments": { "count": 1 } })),
        )
        .await;

        let error = dispatch.response.unwrap().error.unwrap();
        assert_eq!(error.code, error_codes::FORBIDDEN);
    }

    #[tokio::test]
    async fn a_rejected_bearer_token_never_becomes_anonymous() {
        let mut ctx = RequestContext::new();
        ctx.headers
            .insert("authorization".to_string(), vec!["Basic nope".to_string()]);

        let dispatch = McpEngine::dispatch(ctx, None, request("ping", json!({}))).await;

        let error = dispatch.response.unwrap().error.unwrap();
        assert_eq!(error.code, error_codes::UNAUTHENTICATED);
        assert_eq!(dispatch.http_status, 401);
    }

    #[test]
    fn parse_rejects_empty_and_malformed_bodies() {
        assert_eq!(parse_message(b"").unwrap_err().code, error_codes::INVALID_REQUEST);
        assert_eq!(parse_message(b"{not json").unwrap_err().code, error_codes::PARSE_ERROR);
        assert!(parse_message(br#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#).is_ok());
    }
}