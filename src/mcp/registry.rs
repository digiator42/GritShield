use crate::mcp::protocol::McpError;
use crate::mcp::prompt::McpPromptRegistration;
use crate::mcp::resource::McpResourceRegistration;
use crate::mcp::schema::McpToolSchema;
use crate::mcp::tool::{resolve_schema, McpTool, McpToolFn, McpToolRegistration};
use crate::routing::engine::RequestContext;
use dashmap::DashMap;
use serde::Serialize;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Live, mutable state for one tool — the kill switch lives here.
#[derive(Debug, Clone, Serialize)]
pub struct McpToolState {
    /// Whether the tool may currently be invoked.
    pub enabled: bool,
    pub invocations: u64,
    pub failures: u64,
    pub denied: u64,
    pub last_invoked_at: Option<String>,
    pub last_error: Option<String>,
}

/// Mutable counters, kept apart from the immutable registration so the admin UI
/// can toggle and count without contending on tool construction.
#[derive(Debug, Default)]
pub struct ToolCounters {
    invocations: AtomicU64,
    failures: AtomicU64,
    denied: AtomicU64,
    last_invoked_at: Mutex<Option<String>>,
    last_error: Mutex<Option<String>>,
}

impl ToolCounters {
    pub fn snapshot(&self, enabled: bool) -> McpToolState {
        McpToolState {
            enabled,
            invocations: self.invocations.load(Ordering::Relaxed),
            failures: self.failures.load(Ordering::Relaxed),
            denied: self.denied.load(Ordering::Relaxed),
            last_invoked_at: self.last_invoked_at.lock().ok().and_then(|g| g.clone()),
            last_error: self.last_error.lock().ok().and_then(|g| g.clone()),
        }
    }

    fn record_success(&self) {
        self.invocations.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut slot) = self.last_invoked_at.lock() {
            *slot = Some(chrono::Local::now().to_rfc3339());
        }
        if let Ok(mut slot) = self.last_error.lock() {
            *slot = None;
        }
    }

    fn record_failure(&self, message: &str) {
        self.invocations.fetch_add(1, Ordering::Relaxed);
        self.failures.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut slot) = self.last_invoked_at.lock() {
            *slot = Some(chrono::Local::now().to_rfc3339());
        }
        if let Ok(mut slot) = self.last_error.lock() {
            *slot = Some(message.to_string());
        }
    }

    fn record_denial(&self) {
        self.denied.fetch_add(1, Ordering::Relaxed);
    }
}

enum ToolInvoker {
    /// A `#[mcp_tool]` function pointer.
    Static(McpToolFn),
    /// A hand-written `McpTool` implementation.
    Dynamic(Arc<dyn McpTool>),
}

/// The resolved, queryable catalogue of everything the MCP server exposes.
pub struct McpRegistry {
    tools: DashMap<String, Arc<ToolSlot>>,
    resources: Vec<&'static McpResourceRegistration>,
    prompts: Vec<&'static McpPromptRegistration>,
}

/// A single addressable tool.
pub struct ToolSlot {
    pub schema: McpToolSchema,
    pub service: String,
    invoker: ToolInvoker,
    counters: ToolCounters,
    /// The admin kill switch. Starts at the tool's declared default and is
    /// thereafter decoupled from it, so toggling survives a re-read of the
    /// inventory.
    enabled: AtomicBool,
}

impl McpRegistry {
    /// Build the registry from the compile-time inventory.
    pub fn bootstrap() -> Self {
        let mut registry = Self {
            tools: DashMap::new(),
            resources: Vec::new(),
            prompts: Vec::new(),
        };

        for registration in inventory::iter::<McpToolRegistration> {
            registry.insert_static(registration);
        }

        for registration in inventory::iter::<McpResourceRegistration> {
            registry.resources.push(registration);
        }

        for registration in inventory::iter::<McpPromptRegistration> {
            registry.prompts.push(registration);
        }

        // Stable ordering so `tools/list` output is deterministic; clients and
        // snapshot tests both depend on it.
        registry
            .resources
            .sort_by_key(|resource| resource.uri);
        registry.prompts.sort_by_key(|prompt| prompt.name);

        registry
    }

    fn insert_static(&self, registration: &'static McpToolRegistration) {
        let schema = resolve_schema(registration);

        self.tools.insert(
            schema.name.clone(),
            Arc::new(ToolSlot {
                enabled: AtomicBool::new(registration.enabled_by_default),
                schema,
                service: registration.service.to_string(),
                invoker: ToolInvoker::Static(registration.handler),
                counters: ToolCounters::default(),
            }),
        );
    }

    /// Register a hand-written tool at runtime.
    ///
    /// Needed for tools carrying captured state or built from configuration,
    /// which cannot be expressed in the const inventory.
    pub fn register_tool(&self, tool: Arc<dyn McpTool>) {
        let schema = tool.schema();

        self.tools.insert(
            schema.name.clone(),
            Arc::new(ToolSlot {
                enabled: AtomicBool::new(schema.enabled),
                schema,
                service: "runtime".to_string(),
                invoker: ToolInvoker::Dynamic(tool),
                counters: ToolCounters::default(),
            }),
        );
    }

    pub fn tool(&self, name: &str) -> Option<Arc<ToolSlot>> {
        self.tools.get(name).map(|slot| Arc::clone(slot.value()))
    }

    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }

    pub fn resource_count(&self) -> usize {
        self.resources.len()
    }

    pub fn prompt_count(&self) -> usize {
        self.prompts.len()
    }

    /// Every tool name, sorted.
    pub fn tool_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tools.iter().map(|e| e.key().clone()).collect();
        names.sort();
        names
    }

    /// Tool names the caller is permitted to see and invoke.
    ///
    /// Discovery is filtered, not just execution: advertising a tool the caller
    /// cannot use wastes context window and invites a guaranteed-denied call.
    pub fn visible_tool_names(&self, ctx: &RequestContext) -> Vec<String> {
        self.tools
            .iter()
            .filter(|entry| is_permitted(&entry.value().schema, ctx))
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// A snapshot row for the admin capability manager.
    pub fn admin_rows(&self) -> Vec<McpToolAdminRow> {
        let mut rows: Vec<McpToolAdminRow> = self
            .tools
            .iter()
            .map(|entry| {
                let slot = entry.value();
                let enabled = slot.is_enabled();

                McpToolAdminRow {
                    name: slot.schema.name.clone(),
                    service: slot.service.clone(),
                    required_role: slot.schema.required_role,
                    enabled,
                    counters: slot.counters.snapshot(enabled),
                    description: slot.schema.description.clone(),
                }
            })
            .collect();

        rows.sort_by(|a, b| a.name.cmp(&b.name));
        rows
    }

    /// Flip the kill switch for one tool.
    pub fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), McpError> {
        let slot = self
            .tool(name)
            .ok_or_else(|| McpError::NotFound(format!("Unknown MCP tool '{}'", name)))?;

        slot.enabled
            .store(enabled, Ordering::Relaxed);
        Ok(())
    }

    /// Whether the admin kill switch is on.
    pub fn is_enabled(&self, name: &str) -> bool {
        self.tool(name).is_some_and(|slot| slot.is_enabled())
    }

    pub fn resources(&self) -> &[&'static McpResourceRegistration] {
        &self.resources
    }

    pub fn prompts(&self) -> &[&'static McpPromptRegistration] {
        &self.prompts
    }

    pub fn resource(&self, uri: &str) -> Option<&'static McpResourceRegistration> {
        self.resources.iter().copied().find(|r| r.uri == uri)
    }

    pub fn prompt(&self, name: &str) -> Option<&'static McpPromptRegistration> {
        self.prompts.iter().copied().find(|p| p.name == name)
    }
}

impl ToolSlot {
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn state(&self) -> McpToolState {
        self.counters.snapshot(self.is_enabled())
    }

    /// Attribute a completed invocation to this tool's counters.
    pub fn record_success(&self) {
        self.counters.record_success();
    }

    /// Attribute a failed invocation, remembering the message for the UI.
    pub fn record_failure(&self, message: &str) {
        self.counters.record_failure(message);
    }

    /// Attribute a call that was blocked by the kill switch or by RBAC.
    pub fn record_denial(&self) {
        self.counters.record_denial();
    }

    /// Invoke the tool's handler.
    ///
    /// The caller is responsible for having already checked the kill switch and
    /// the caller's role; this is the last mile.
    pub async fn invoke(
        &self,
        ctx: RequestContext,
        args: Value,
    ) -> Result<Value, McpError> {
        match &self.invoker {
            ToolInvoker::Static(handler) => handler(ctx, args).await,
            ToolInvoker::Dynamic(tool) => {
                tool.execute(args, &ctx).await.map_err(McpError::Execution)
            }
        }
    }
}

/// Whether `ctx` holds the role a schema requires.
pub fn is_permitted(schema: &McpToolSchema, ctx: &RequestContext) -> bool {
    match schema.required_role {
        None => true,
        Some(role) => ctx.has_role(role),
    }
}

/// One row of the admin capability manager table.
#[derive(Debug, Clone, Serialize)]
pub struct McpToolAdminRow {
    pub name: String,
    pub service: String,
    pub required_role: Option<&'static str>,
    pub enabled: bool,
    pub counters: McpToolState,
    pub description: String,
}

lazy_static::lazy_static! {
    static ref MCP_REGISTRY: McpRegistry = McpRegistry::bootstrap();
}

/// The process-wide capability registry.
pub fn registry() -> &'static McpRegistry {
    &MCP_REGISTRY
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::schema::empty_object_schema;
    use sea_orm_migration::async_trait::async_trait;

    struct Ping {
        role: Option<&'static str>,
    }

    #[async_trait]
    impl McpTool for Ping {
        fn schema(&self) -> McpToolSchema {
            let schema = McpToolSchema::new("ping_tool", "Ping", empty_object_schema());
            match self.role {
                Some(role) => schema.requiring_role(role),
                None => schema,
            }
        }

        async fn execute(&self, _args: Value, _ctx: &RequestContext) -> Result<Value, String> {
            Ok(serde_json::json!({ "pong": true }))
        }
    }

    fn ctx_with_role(role: &str) -> RequestContext {
        let mut ctx = RequestContext::new();
        ctx.role_inheritance = Arc::new(
            [(role.to_string(), vec!["Sub".to_string()])]
                .into_iter()
                .collect(),
        );
        // `get_user_role` reads the session first, so seed a session-backed role.
        let session = crate::security::session::Session {
            id: uuid::Uuid::new_v4().to_string(),
            data: [("role".to_string(), role.to_string())].into_iter().collect(),
            user_id: Some("tester".to_string()),
            last_accessed: std::time::Instant::now(),
        };
        ctx.session = Some(Arc::new(Mutex::new(session)));
        ctx
    }

    #[test]
    fn runtime_tools_are_registered_and_discoverable() {
        let registry = McpRegistry::bootstrap();
        registry.register_tool(Arc::new(Ping { role: None }));

        assert!(registry.tool("ping_tool").is_some());
        assert!(registry.tool_names().contains(&"ping_tool".to_string()));
    }

    #[test]
    fn the_kill_switch_flips_without_touch_the_registration() {
        let registry = McpRegistry::bootstrap();
        registry.register_tool(Arc::new(Ping { role: None }));

        assert!(registry.is_enabled("ping_tool"));
        registry.set_enabled("ping_tool", false).unwrap();
        assert!(!registry.is_enabled("ping_tool"));

        registry.set_enabled("ping_tool", true).unwrap();
        assert!(registry.is_enabled("ping_tool"));
    }

    #[test]
    fn toggling_an_unknown_tool_is_an_error_not_a_panic() {
        let registry = McpRegistry::bootstrap();
        assert!(matches!(
            registry.set_enabled("nope", false),
            Err(McpError::NotFound(_))
        ));
    }

    #[test]
    fn role_gated_tools_are_hidden_from_unauthorized_discovery() {
        let registry = McpRegistry::bootstrap();
        registry.register_tool(Arc::new(Ping { role: Some("Admin") }));

        let viewer = ctx_with_role("Viewer");
        let visible = registry.visible_tool_names(&viewer);

        assert!(
            !visible.contains(&"ping_tool".to_string()),
            "a role-gated tool must not be advertised to a caller who cannot use it"
        );
    }

    #[tokio::test]
    async fn counters_track_invocations_and_failures() {
        let registry = McpRegistry::bootstrap();
        registry.register_tool(Arc::new(Ping { role: None }));

        let slot = registry.tool("ping_tool").unwrap();
        let ctx = RequestContext::new();

        slot.invoke(ctx.clone(), serde_json::json!({})).await.unwrap();
        slot.counters.record_success();

        let row = registry.admin_rows();
        let ping = row.iter().find(|r| r.name == "ping_tool").unwrap();
        assert_eq!(ping.counters.invocations, 1);
        assert_eq!(ping.counters.failures, 0);
    }
}