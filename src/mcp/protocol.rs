use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The only JSON-RPC version this server speaks.
pub const JSONRPC_VERSION: &str = "2.0";

/// The MCP protocol revision this server implements.
pub const PROTOCOL_VERSION: &str = "2025-06-18";

/// Revisions accepted during `initialize` negotiation.
///
/// A client asking for anything outside this set is not silently downgraded —
/// it gets an explicit `initialize` failure, because proceeding under the wrong
/// revision produces baffling errors much later.
pub const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-06-18", "2025-03-26", "2024-11-05"];

/// Server identity reported in the `initialize` handshake.
pub const SERVER_NAME: &str = "gritshield";

pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// JSON-RPC method names for the MCP surface.
pub mod methods {
    pub const INITIALIZE: &str = "initialize";
    pub const INITIALIZED: &str = "notifications/initialized";
    pub const PING: &str = "ping";

    pub const TOOLS_LIST: &str = "tools/list";
    pub const TOOLS_CALL: &str = "tools/call";
    pub const TOOLS_LIST_CHANGED: &str = "notifications/tools/list_changed";

    pub const RESOURCES_LIST: &str = "resources/list";
    pub const RESOURCES_READ: &str = "resources/read";
    pub const RESOURCES_TEMPLATES_LIST: &str = "resources/templates/list";
    pub const RESOURCES_LIST_CHANGED: &str = "notifications/resources/list_changed";

    pub const PROMPTS_LIST: &str = "prompts/list";
    pub const PROMPTS_GET: &str = "prompts/get";
    pub const PROMPTS_LIST_CHANGED: &str = "notifications/prompts/list_changed";

    pub const LOGGING_SET_LEVEL: &str = "logging/setLevel";
}

/// JSON-RPC and MCP error codes.
///
/// `-32768..=-32000` is the implementation-defined server range, which is where
/// the MCP-specific failures (tool disabled, RBAC denial) live. The four
/// transport-level codes are reserved by the JSON-RPC 2.0 spec.
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    /// The kill switch is off for this tool.
    pub const TOOL_DISABLED: i32 = -32001;
    /// The caller lacks the role this tool requires.
    pub const FORBIDDEN: i32 = -32002;
    /// The supplied arguments failed JSON Schema pre-flight validation.
    pub const SCHEMA_VIOLATION: i32 = -32003;
    /// The caller is not authenticated.
    pub const UNAUTHENTICATED: i32 = -32004;
    /// The named tool / resource / prompt does not exist.
    pub const NOT_FOUND: i32 = -32005;
    /// The handler ran but returned an error.
    pub const EXECUTION_FAILED: i32 = -32006;
    /// The request referenced an unknown or expired session.
    pub const UNKNOWN_SESSION: i32 = -32007;
}

/// A JSON-RPC request or notification.
///
/// `id` being absent is what makes a message a *notification*: the server
/// executes it and returns nothing. `id: null` is a distinct, valid request id.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    /// `Some(Value::Null)` for an explicit `"id": null`, `None` when absent.
    ///
    /// Plain `Option<Value>` cannot express that difference — serde maps a JSON
    /// `null` onto `None` — and getting it wrong turns a valid request into a
    /// silently dropped one, so presence is recovered during deserialization.
    #[serde(default, deserialize_with = "deserialize_id")]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Option<Value>,
}

/// Deserialize an `id` member while preserving the absent/null distinction.
fn deserialize_id<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Reached only when the member is present, so wrapping whatever arrives —
    // including `null` — in `Some` is exactly the presence signal we need.
    Ok(Some(Value::deserialize(deserializer)?))
}

impl JsonRpcRequest {
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }

    /// Validate the envelope fields the spec pins down.
    pub fn validate_envelope(&self) -> Result<(), McpError> {
        if self.jsonrpc != JSONRPC_VERSION {
            return Err(McpError::InvalidRequest(format!(
                "Unsupported JSON-RPC version '{}'; expected '{}'",
                self.jsonrpc, JSONRPC_VERSION
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn failure(id: Value, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }

    /// Collapse the response into a plain result.
    ///
    /// A response carries exactly one of `result`/`error`, so callers that only
    /// care about the happy path (tests, and the stdio loop) should not have to
    /// assert on both fields by hand.
    pub fn into_result(self) -> Result<Value, JsonRpcError> {
        match self.error {
            Some(error) => Err(error),
            None => Ok(self.result.unwrap_or(Value::Null)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// Every failure mode the MCP layer can produce, and its mapping onto a
/// JSON-RPC error code.
///
/// Handlers written against the SDK return `Result<Value, McpError>`, so the
/// distinction between "the request was malformed", "you are not allowed", and
/// "your business logic threw" survives all the way to the wire. Collapsing
/// those into one opaque string is exactly the failure mode that makes MCP
/// servers miserable to debug.
#[derive(Debug, Clone)]
pub enum McpError {
    /// The JSON envelope itself was invalid.
    InvalidRequest(String),
    /// `tools/call` referenced a tool that is not registered.
    NotFound(String),
    /// The kill switch is off for this tool.
    Disabled(String),
    /// The caller is not authenticated.
    Unauthenticated(String),
    /// The caller is authenticated but lacks the required role.
    Forbidden(String),
    /// Arguments failed JSON Schema pre-flight validation.
    SchemaViolation(String),
    /// Arguments were structurally wrong (e.g. not an object).
    InvalidParams(String),
    /// The tool ran and failed.
    Execution(String),
    /// The session is missing or expired.
    UnknownSession(String),
    /// A framework fault.
    Internal(String),
}

impl McpError {
    pub fn code(&self) -> i32 {
        match self {
            McpError::InvalidRequest(_) => error_codes::INVALID_REQUEST,
            McpError::NotFound(_) => error_codes::NOT_FOUND,
            McpError::Disabled(_) => error_codes::TOOL_DISABLED,
            McpError::Unauthenticated(_) => error_codes::UNAUTHENTICATED,
            McpError::Forbidden(_) => error_codes::FORBIDDEN,
            McpError::SchemaViolation(_) => error_codes::SCHEMA_VIOLATION,
            McpError::InvalidParams(_) => error_codes::INVALID_PARAMS,
            McpError::Execution(_) => error_codes::EXECUTION_FAILED,
            McpError::UnknownSession(_) => error_codes::UNKNOWN_SESSION,
            McpError::Internal(_) => error_codes::INTERNAL_ERROR,
        }
    }

    /// The `data` payload attached to the JSON-RPC error, when there is
    /// structured detail worth giving the model beyond the message string.
    pub fn data(&self) -> Option<Value> {
        match self {
            McpError::SchemaViolation(detail) => Some(json!({ "violations": detail })),
            McpError::Disabled(detail) => Some(json!({ "reason": detail })),
            _ => None,
        }
    }

    pub fn to_json_rpc(&self) -> JsonRpcError {
        let mut err = JsonRpcError::new(self.code(), self.to_string());
        if let Some(data) = self.data() {
            err = err.with_data(data);
        }
        err
    }
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            McpError::InvalidRequest(m)
            | McpError::NotFound(m)
            | McpError::Disabled(m)
            | McpError::Unauthenticated(m)
            | McpError::Forbidden(m)
            | McpError::SchemaViolation(m)
            | McpError::InvalidParams(m)
            | McpError::Execution(m)
            | McpError::UnknownSession(m)
            | McpError::Internal(m) => write!(f, "{}", m),
        }
    }
}

impl std::error::Error for McpError {}

/// Ergonomic so a handler can bubble a plain `Err(String)` up into the engine.
impl From<String> for McpError {
    fn from(value: String) -> Self {
        McpError::Execution(value)
    }
}

impl From<&str> for McpError {
    fn from(value: &str) -> Self {
        McpError::Execution(value.to_string())
    }
}

impl From<serde_json::Error> for McpError {
    fn from(value: serde_json::Error) -> Self {
        McpError::Internal(format!("JSON serialization fault: {}", value))
    }
}

impl std::fmt::Display for JsonRpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl std::error::Error for JsonRpcError {}

/// Build a `content` block array from a JSON result.
///
/// MCP requires every tool result to carry at least one content block, even
/// when the payload is purely structured. Returning the JSON inline as text
/// plus the typed `structuredContent` gives older clients (which only read
/// text) something usable while newer ones get the typed value.
pub fn content_from_value(value: &Value) -> Vec<Value> {
    let text = match value {
        Value::String(s) => s.clone(),
        other => serde_json::to_string_pretty(other).unwrap_or_else(|_| other.to_string()),
    };

    vec![json!({ "type": "text", "text": text })]
}

/// Assemble the `tools/call` success payload.
pub fn tool_success(value: Value) -> Value {
    let structured = match &value {
        Value::Object(map) if !map.is_empty() => Some(value.clone()),
        _ => None,
    };

    let mut result = Map::new();
    result.insert("content".to_string(), Value::Array(content_from_value(&value)));
    result.insert("isError".to_string(), Value::Bool(false));
    if let Some(structured) = structured {
        result.insert("structuredContent".to_string(), structured);
    }
    Value::Object(result)
}

/// Assemble the `tools/call` failure payload.
///
/// Per the MCP spec a tool that ran and failed reports `isError: true` in a
/// *successful* JSON-RPC response. That is deliberate: the model gets to see
/// what went wrong and retry, rather than the transport collapsing the
/// distinction and the model treating it as a dead connection.
pub fn tool_failure(message: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": message }],
        "isError": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_success_wraps_scalars_as_text() {
        let out = tool_success(json!({ "id": 7 }));
        assert_eq!(out["isError"], json!(false));
        assert_eq!(out["content"][0]["type"], json!("text"));
        assert_eq!(out["structuredContent"], json!({ "id": 7 }));
    }

    #[test]
    fn tool_success_omits_structured_content_for_scalars() {
        let out = tool_success(json!("plain"));
        assert!(out.get("structuredContent").is_none());
        assert_eq!(out["content"][0]["text"], json!("plain"));
    }

    #[test]
    fn errors_map_to_distinct_codes() {
        assert_eq!(McpError::Disabled("x".into()).code(), error_codes::TOOL_DISABLED);
        assert_eq!(McpError::Forbidden("x".into()).code(), error_codes::FORBIDDEN);
        assert_ne!(
            McpError::Execution("x".into()).code(),
            McpError::Internal("x".into()).code()
        );
    }

    #[test]
    fn notifications_are_detected_by_absent_id() {
        let notif: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .unwrap();
        assert!(notif.is_notification());

        let request: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#).unwrap();
        assert!(!request.is_notification());
    }

    #[test]
    fn wrong_jsonrpc_version_is_rejected() {
        let req: JsonRpcRequest =
            serde_json::from_str(r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#).unwrap();
        assert!(req.validate_envelope().is_err());
    }
}