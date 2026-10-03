use crate::mcp::protocol::McpError;
use crate::mcp::tool::McpBoxedFuture;
use crate::routing::engine::RequestContext;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use sea_orm_migration::async_trait::async_trait;

/// The erased async signature for a resource reader.
pub type McpResourceFn =
    fn(RequestContext) -> McpBoxedFuture<Result<McpResourceContents, McpError>>;

/// The body of a single resource read.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResourceContents {
    pub uri: String,
    #[serde(rename = "mimeType")]
    pub mime_type: String,
    pub text: String,
}

impl McpResourceContents {
    pub fn text(uri: &str, text: impl Into<String>) -> Self {
        Self {
            uri: uri.to_string(),
            mime_type: "text/plain".to_string(),
            text: text.into(),
        }
    }

    pub fn json(uri: &str, value: &Value) -> Self {
        Self {
            uri: uri.to_string(),
            mime_type: "application/json".to_string(),
            text: serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
        }
    }
}

/// The read-only data stream abstraction (diagnostics, logs, metrics).
#[async_trait]
pub trait McpResource: Send + Sync {
    fn uri(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn mime_type(&self) -> &'static str {
        "text/plain"
    }

    /// Produce the current contents of the stream.
    async fn read(&self, ctx: &RequestContext) -> Result<String, McpError>;
}

/// A compile-time resource registration submitted by `#[mcp_resource]`.
pub struct McpResourceRegistration {
    pub uri: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub mime_type: &'static str,
    pub required_role: Option<&'static str>,
    pub reader: McpResourceFn,
}

inventory::collect!(McpResourceRegistration);

impl McpResourceRegistration {
    /// The `resources/list` descriptor for this resource.
    pub fn to_descriptor(&self) -> Value {
        let mut descriptor = json!({
            "uri": self.uri,
            "name": self.name,
            "description": self.description,
            "mimeType": self.mime_type,
        });
        if let Some(role) = self.required_role {
            descriptor["_gritshield"] = json!({ "requiredRole": role });
        }
        descriptor
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptors_expose_the_wire_field_names() {
        let reg = McpResourceRegistration {
            uri: "gritshield://system/diagnostics",
            name: "Diagnostics",
            description: "Live framework counters",
            mime_type: "application/json",
            required_role: Some("Admin"),
            reader: |_ctx| Box::pin(async { Ok(McpResourceContents::text("", "ok")) }),
        };

        let descriptor = reg.to_descriptor();
        assert_eq!(descriptor["uri"], json!("gritshield://system/diagnostics"));
        assert_eq!(descriptor["mimeType"], json!("application/json"));
        assert_eq!(descriptor["_gritshield"]["requiredRole"], json!("Admin"));
    }

    #[test]
    fn json_contents_are_pretty_printed_with_the_json_mime() {
        let contents = McpResourceContents::json("gritshield://x", &json!({ "a": 1 }));
        assert_eq!(contents.mime_type, "application/json");
        assert!(contents.text.contains("\"a\": 1"));
    }
}