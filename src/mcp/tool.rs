use crate::mcp::protocol::McpError;
use crate::mcp::schema::McpToolSchema;
use crate::routing::engine::RequestContext;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

use sea_orm_migration::async_trait::async_trait;

pub type McpBoxedFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// The erased async signature every compile-time tool handler is adapted to.
///
/// This signature is what makes compile-time registration possible:
/// `inventory::submit!` requires a fully `const`-constructible value, and an
/// `async fn` item coerces to a function pointer in that position, while a
/// boxed closure or an `Arc<dyn McpTool>` does not.
pub type McpToolFn = fn(RequestContext, Value) -> McpBoxedFuture<Result<Value, McpError>>;

/// The core abstraction for anything an AI assistant can invoke.
///
/// Prefer the `#[mcp_tool]` attribute over hand-implementing this: the macro
/// generates the inventory submission, argument deserialization and `Value`
/// marshalling that would otherwise be boilerplate for every tool. Implement
/// the trait directly only for tools that need captured state, generics, or a
/// runtime-constructed (non-compile-time) registration.
#[async_trait]
pub trait McpTool: Send + Sync {
    /// The declarative contract advertised to clients.
    fn schema(&self) -> McpToolSchema;

    /// Run the tool.
    ///
    /// The framework has already validated `args` against
    /// [`schema().input_schema`](crate::mcp::schema::McpToolSchema::input_schema)
    /// and checked the caller's RBAC role by the time this is reached, so an
    /// implementation can trust its arguments.
    async fn execute(&self, args: Value, ctx: &RequestContext) -> Result<Value, String>;
}

/// A compile-time tool registration submitted by `#[mcp_tool]`.
///
/// Every field is `const`-constructible — no `String`, no `Vec`, no `Arc` — so
/// the whole struct can live in the inventory's statically linked list.
pub struct McpToolRegistration {
    /// The MCP tool name, unique across the server.
    pub name: &'static str,
    /// Purpose statement shown to the model during tool selection.
    pub description: &'static str,
    /// Owning service or subsystem, surfaced in the admin capability manager.
    pub service: &'static str,
    /// Produces the fully-resolved schema. A function rather than a `Value`
    /// because the schema is assembled at runtime, not in a const context.
    pub input_schema: fn() -> Value,
    /// The GritShield role a caller must hold. `None` means unrestricted.
    pub required_role: Option<&'static str>,
    /// Whether the tool starts live, before any admin toggle.
    pub enabled_by_default: bool,
    /// The adapted handler.
    pub handler: McpToolFn,
}

inventory::collect!(McpToolRegistration);

/// Fully resolve a registration into the schema clients and guards see.
pub fn resolve_schema(reg: &McpToolRegistration) -> McpToolSchema {
    let mut schema = McpToolSchema::new(reg.name, reg.description, (reg.input_schema)());
    schema.required_role = reg.required_role;
    schema.enabled = reg.enabled_by_default;
    schema
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::schema::empty_object_schema;
    use crate::mcp::schema::validate;

    struct Echo;

    #[async_trait]
    impl McpTool for Echo {
        fn schema(&self) -> McpToolSchema {
            McpToolSchema::new("echo", "Echoes its input", empty_object_schema())
        }

        async fn execute(&self, args: Value, _ctx: &RequestContext) -> Result<Value, String> {
            Ok(args)
        }
    }

    #[tokio::test]
    async fn trait_tools_get_schema_preflight_before_execute() {
        let tool = Echo;
        let schema = tool.schema();

        let checked = validate(&schema.input_schema, &serde_json::json!({ "a": 1 }));
        assert!(checked.is_valid());

        let out = tool.execute(checked.value, &RequestContext::new()).await;
        assert_eq!(out.unwrap(), serde_json::json!({ "a": 1 }));
    }
}