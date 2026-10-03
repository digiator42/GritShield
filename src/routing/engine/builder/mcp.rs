use colored::Colorize;

use crate::mcp::transport::{
    self, handle_message, handle_server_info, handle_sse, handle_streamable, handle_terminate,
    DEFAULT_MCP_PREFIX,
};
use crate::routing::engine::Router;

/// The mount point for the MCP HTTP surface, overridable per application.
///
/// `MCP_HTTP_PREFIX=agent` serves the same surface under `/agent` instead of
/// `/mcp`. Surrounding slashes are normalised so both spellings behave the same.
pub fn prefix() -> String {
    let configured =
        std::env::var("MCP_HTTP_PREFIX").unwrap_or_else(|_| DEFAULT_MCP_PREFIX.to_string());
    let trimmed = configured.trim().trim_matches('/');
    if trimmed.is_empty() {
        format!("/{}", DEFAULT_MCP_PREFIX.trim_matches('/'))
    } else {
        format!("/{}", trimmed)
    }
}

impl Router {
    /// Mount the MCP HTTP transport on the router trie.
    ///
    /// Endpoints are registered without a router-level `required_role` on
    /// purpose: authorisation in MCP is per capability, so one caller may be
    /// allowed to list prompts while being denied a specific tool. `McpEngine`
    /// applies the per-capability RBAC check on every request.
    pub(crate) fn register_mcp_routes(&mut self) {
        let prefix = prefix();
        let routes = transport::route_table(&prefix);
        let max_len = routes
            .iter()
            .map(|(_, _, path)| path.len())
            .max()
            .unwrap_or(0);

        for (name, method, path) in routes {
            crate::debug!(
                "[MCP] >>: {0:<1$} -> [{2:<6}]",
                path,
                max_len,
                format!("{:?}", method).green()
            );

            // Each arm registers independently: the handlers are distinct
            // `async fn` items and reach `add_route` through the blanket
            // `IntoHandler` impl rather than a single shared type.
            match name {
                "sse" => self.add_route(method, &path, handle_sse, None),
                "message" => self.add_route(method, &path, handle_message, None),
                "streamable" => self.add_route(method, &path, handle_streamable, None),
                "terminate" => self.add_route(method, &path, handle_terminate, None),
                _ => self.add_route(method, &path, handle_server_info, None),
            };
        }
    }
}