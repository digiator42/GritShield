use crate::core::env::get_env;
use crate::mcp::engine::{parse_message, McpEngine};
use crate::mcp::protocol::JsonRpcResponse;
use crate::routing::engine::RequestContext;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Build the context a stdio message runs under.
///
/// stdio carries no HTTP request, so the caller identity comes from the
/// environment instead. `MCP_STDIO_ROLE` exists because a stdio transport is
/// normally spoken to by a local agent the operator already trusts; RBAC still
/// applies, so that role is a *ceiling*, not a bypass.
fn stdio_context() -> RequestContext {
    let mut ctx = RequestContext::new();

    let role = get_env("MCP_STDIO_ROLE", "SuperAdmin");
    let subject = get_env("MCP_STDIO_SUBJECT", "stdio");

    ctx.session = Some(Arc::new(Mutex::new(crate::security::session::Session {
        id: uuid::Uuid::new_v4().to_string(),
        data: [
            ("role".to_string(), role),
            ("user_id".to_string(), subject.clone()),
            ("transport".to_string(), "stdio".to_string()),
        ]
        .into_iter()
        .collect(),
        user_id: Some(subject),
        last_accessed: std::time::Instant::now(),
    })));

    ctx
}

/// Handle one newline-delimited JSON-RPC message, returning the reply to write.
///
/// Returns `None` for notifications, which by spec get no response.
pub async fn handle_line(line: &str) -> Option<JsonRpcResponse> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }

    // NDJSON clients often prefix the message, and LSP-style transports wrap it
    // in Content-Length headers; tolerate neither silently failing.
    let request = match parse_message(trimmed.as_bytes()) {
        Ok(request) => request,
        Err(error) => {
            return Some(JsonRpcResponse::failure(Value::Null, error));
        }
    };

    let dispatch = McpEngine::dispatch(stdio_context(), None, request).await;
    dispatch.response
}

/// Serve MCP over stdin/stdout, one JSON document per line.
///
/// stdout is reserved for protocol traffic, so every diagnostic goes to stderr —
/// a stray log line on stdout would corrupt the client's parse stream.
pub async fn serve_stdio() -> std::io::Result<()> {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut stdout = tokio::io::stdout();

    let mut lines = stdin.lines();

    eprintln!(
        "[MCP] stdio transport ready — {} tools, {} resources, {} prompts",
        crate::mcp::registry::registry().tool_count(),
        crate::mcp::registry::registry().resource_count(),
        crate::mcp::registry::registry().prompt_count(),
    );

    while let Some(line) = lines.next_line().await? {
        if let Some(response) = handle_line(&line).await {
            let mut payload = serde_json::to_string(&response)?;
            payload.push('\n');
            stdout.write_all(payload.as_bytes()).await?;
            stdout.flush().await?;
        }
    }

    Ok(())
}

/// Block on stdio until the client disconnects.
///
/// Provided for embedding in a synchronous `main`.
pub fn serve_stdio_blocking() -> std::io::Result<()> {
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(serve_stdio())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_ping_is_answered() {
        let response = handle_line(r#"{"jsonrpc":"2.0","id":7,"method":"ping"}"#)
            .await
            .expect("ping should produce a response");

        assert_eq!(response.id, serde_json::json!(7));
        assert!(response.error.is_none());
    }

    #[tokio::test]
    async fn blank_lines_are_skipped() {
        assert!(handle_line("   ").await.is_none());
    }

    #[tokio::test]
    async fn notifications_produce_no_output() {
        let response = handle_line(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).await;
        assert!(response.is_none(), "notifications must stay silent on stdio");
    }

    #[tokio::test]
    async fn malformed_input_yields_a_parse_error_response() {
        let response = handle_line("{oops").await.expect("parse errors are reportable");
        assert_eq!(
            response.error.unwrap().code,
            crate::mcp::protocol::error_codes::PARSE_ERROR
        );
    }

    #[tokio::test]
    async fn initialize_reports_the_negotiated_revision() {
        let response = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#,
        )
        .await
        .unwrap();

        let result = response.result.unwrap();
        assert_eq!(result["protocolVersion"], serde_json::json!("2025-06-18"));
    }
}