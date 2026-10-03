use crate::core::env::get_env;
use crate::http::response::Response;
use crate::http::sse::SseStream;
use crate::mcp::audit::{self, outcome};
use crate::mcp::auth::{self, McpAuthOutcome};
use crate::mcp::engine::{parse_message, McpDispatch, McpEngine};
use crate::mcp::protocol::{JsonRpcResponse, McpError};
use crate::mcp::registry::registry;
use crate::mcp::session::{sessions, McpSession};
use crate::routing::engine::RequestContext;
use serde_json::{json, Value};
use std::sync::Arc;

/// The path prefix the MCP HTTP transport is mounted at.
pub const DEFAULT_MCP_PREFIX: &str = "/mcp";

/// Whether a request with no `session_id` may still be served.
///
/// The Streamable HTTP transport is stateless by design, so this defaults to
/// permissive — it makes `curl` against a tool genuinely usable. The SSE
/// transport still requires its session, because that handshake exists precisely
/// to establish one.
fn allow_stateless() -> bool {
    !matches!(
        get_env("MCP_ALLOW_STATELESS", "true").to_ascii_lowercase().as_str(),
        "0" | "false" | "no" | "off"
    )
}

/// Reject a request that failed authentication, in the shape a transport expects.
fn reject(outcome_value: &McpAuthOutcome) -> Response {
    match outcome_value {
        McpAuthOutcome::Rejected { status, reason } => {
            let mapped = match *status {
                401 => crate::http::response::HttpStatus::Unauthorized,
                403 => crate::http::response::HttpStatus::Forbidden,
                _ => crate::http::response::HttpStatus::BadRequest,
            };
            Response::json(
                mapped,
                &json!({
                    "error": reason,
                    "gritshield": "mcp authentication required",
                }),
            )
        }
        McpAuthOutcome::Allowed(_) => {
            Response::json_internal_error_msg("unreachable authentication state")
        }
    }
}

/// `GET /mcp/sse` — open the long-lived event stream.
///
/// The first frame announces the POST endpoint the client must use; every later
/// frame carries a JSON-RPC response. This is the handshake that makes the
/// HTTP+SSE transport work, so it is queued as a guaranteed-first frame rather
/// than broadcast (a broadcast could race the writer's subscription and be lost
/// before the client ever learns its session id).
pub async fn handle_sse(ctx: RequestContext) -> Response {
    let auth = auth::authenticate(&ctx);
    if !auth.is_allowed() {
        if let McpAuthOutcome::Rejected { reason, .. } = &auth {
            audit::audit().record(
                "transport/sse",
                "-",
                "rejected",
                None,
                &ctx.resolve_client_ip(),
                None,
                json!({}),
                outcome::REJECTED,
                Some(reason.clone()),
                0,
            );
        }
        return reject(&auth);
    }

    sessions().prune();
    let session = sessions().create();

    // The stream handed to the response must be the *session's* sender, or
    // frames pushed by `POST /mcp/message` would never reach this connection.
    let mut stream = session.stream();
    stream.queue_initial("endpoint", &session.endpoint_path(DEFAULT_MCP_PREFIX));

    // The response carries a clone; the session stays in the store, keeping its
    // own sender alive until TTL reaping or an explicit DELETE.
    let response = Response::sse(stream);

    crate::info!(
        "[MCP] SSE stream opened for session {} ({} live)",
        &session.id[..8.min(session.id.len())],
        sessions().len()
    );

    response
}

/// `POST /mcp/message?session_id=<ID>` — receive a JSON-RPC request.
///
/// Answers `202 Accepted` and pushes the actual JSON-RPC response down the
/// session's SSE stream, which is what the HTTP+SSE transport requires. When no
/// session id is supplied and stateless mode is on, the response is returned
/// inline instead, so simple clients need no stream at all.
pub async fn handle_message(ctx: RequestContext) -> Response {
    let auth = auth::authenticate(&ctx);
    if !auth.is_allowed() {
        return reject(&auth);
    }

    let session_id = ctx
        .query_param("session_id")
        .map(str::to_string)
        .filter(|id| !id.is_empty());

    match session_id {
        Some(id) => match sessions().get(&id) {
            Some(session) => serve_with_session(ctx, session).await,
            None => {
                audit::audit().record(
                    "transport/message",
                    "-",
                    "rejected",
                    None,
                    &ctx.resolve_client_ip(),
                    Some(id.clone()),
                    json!({}),
                    outcome::REJECTED,
                    Some("Unknown or expired session".to_string()),
                    0,
                );

                Response::json_not_found(&json!({
                    "error": format!("Unknown or expired MCP session '{}'", id),
                }))
            }
        },
        None if allow_stateless() => serve_stateless(ctx).await,
        None => Response::json_bad_request(&json!({
            "error": "Missing 'session_id' query parameter. Open GET /mcp/sse first, \
                      or enable MCP_ALLOW_STATELESS for inline responses."
        })),
    }
}

async fn serve_with_session(ctx: RequestContext, session: Arc<McpSession>) -> Response {
    let message = match parse_message(&ctx.raw_body) {
        Ok(message) => message,
        Err(error) => {
            let response = JsonRpcResponse::failure(Value::Null, error);
            let _ = session.push("message", &serde_json::to_value(&response).unwrap_or(json!({})));
            return accepted();
        }
    };

    let is_notification = message.is_notification();
    let dispatch = McpEngine::dispatch(ctx, Some(session.clone()), message).await;

    match dispatch.response {
        Some(response) => {
            let payload = serde_json::to_value(&response).unwrap_or_else(|_| json!({}));
            let _ = session.push("message", &payload);
        }
        None => {
            crate::debug!(
                "[MCP] Notification '{}' consumed on session {}",
                if is_notification { "ack" } else { "response" },
                &session.id[..8.min(session.id.len())]
            );
        }
    }

    accepted()
}

/// Serve a request with no session, returning the JSON-RPC response inline.
async fn serve_stateless(ctx: RequestContext) -> Response {
    let message = match parse_message(&ctx.raw_body) {
        Ok(message) => message,
        Err(error) => {
            return Response::json(
                crate::http::response::HttpStatus::BadRequest,
                &serde_json::to_value(JsonRpcResponse::failure(Value::Null, error))
                    .unwrap_or_else(|_| json!({})),
            )
        }
    };

    let is_notification = message.is_notification();
    let dispatch = McpEngine::dispatch(ctx, None, message).await;

    match dispatch.response {
        Some(response) => {
            let status = crate::http::response::HttpStatus::Ok;
            match serde_json::to_value(&response) {
                Ok(payload) => Response::json(status, &payload),
                Err(error) => Response::json_internal_error(&json!({ "error": error.to_string() })),
            }
        }
        // A notification on a stateless transport has nowhere to go: acknowledge.
        None if is_notification => accepted(),
        None => accepted(),
    }
}

fn accepted() -> Response {
    Response::json_accepted(&json!({ "status": "accepted" }))
}

/// `POST /mcp` — the Streamable HTTP transport.
///
/// Honours `Accept: text/event-stream` by replying with a single-event stream,
/// and otherwise answers with a plain JSON body. A new session is minted when
/// the client does not present one, and returned in the `Mcp-Session-Id` header.
pub async fn handle_streamable(ctx: RequestContext) -> Response {
    let auth = auth::authenticate(&ctx);
    if !auth.is_allowed() {
        return reject(&auth);
    }

    let presented = ctx
        .header("mcp-session-id")
        .map(str::to_string)
        .filter(|id| !id.is_empty());

    let session = match presented {
        Some(id) => match sessions().get(&id) {
            Some(session) => Some(session),
            None => {
                return Response::json_not_found(&json!({
                    "error": format!("Unknown or expired MCP session '{}'", id),
                    "hint": "POST /mcp without Mcp-Session-Id to open a new session",
                }))
            }
        },
        None => Some(sessions().create()),
    };

    let message = match parse_message(&ctx.raw_body) {
        Ok(message) => message,
        Err(error) => {
            let payload =
                serde_json::to_value(JsonRpcResponse::failure(Value::Null, error)).unwrap_or_default();
            return Response::json(
                crate::http::response::HttpStatus::BadRequest,
                &payload,
            );
        }
    };

    let wants_stream = ctx
        .header("accept")
        .map(|accept| accept.contains("text/event-stream"))
        .unwrap_or(false);

    let dispatch = McpEngine::dispatch(ctx, session.clone(), message).await;

    let mut response = match dispatch.response {
        Some(response) => match serde_json::to_value(&response) {
            Ok(payload) => {
                if wants_stream {
                    let mut stream = SseStream::new();
                    stream.queue_initial("message", &payload.to_string());
                    // No long-lived session is retained for a single-shot
                    // stream, so hand the socket straight back when it closes.
                    Response::sse(stream)
                } else {
                    Response::json_ok(&payload)
                }
            }
            Err(error) => Response::json_internal_error(&json!({ "error": error.to_string() })),
        },
        // Notifications get an empty acknowledgement.
        None => {
            if wants_stream {
                Response::sse(SseStream::new())
            } else {
                Response::json_no_content()
            }
        }
    };

    if let Some(session) = &session {
        response = response.with_header("Mcp-Session-Id", session.id.clone());
    }

    response
}

/// `DELETE /mcp` — explicitly terminate a session.
pub async fn handle_terminate(ctx: RequestContext) -> Response {
    let auth = auth::authenticate(&ctx);
    if !auth.is_allowed() {
        return reject(&auth);
    }

    let Some(id) = ctx
        .header("mcp-session-id")
        .map(str::to_string)
        .filter(|id| !id.is_empty())
    else {
        return Response::json_bad_request(&json!({
            "error": "Missing 'Mcp-Session-Id' header"
        }));
    };

    match sessions().remove(&id) {
        Some(_) => Response::json_ok(&json!({ "terminated": id })),
        None => Response::json_not_found(&json!({ "error": "Unknown MCP session" })),
    }
}

/// `GET /mcp` — server identity, endpoints and live capability counts.
///
/// Not part of the MCP wire protocol; it exists so operators (and the admin
/// config generator) can verify a deployment without speaking JSON-RPC.
pub async fn handle_server_info(_ctx: RequestContext) -> Response {
    let registry = registry();

    let counts = json!({
        "tools": registry.tool_count(),
        "toolsEnabled": registry.tool_names().iter().filter(|n| registry.is_enabled(n)).count(),
        "resources": registry.resource_count(),
        "prompts": registry.prompt_count(),
        "activeSessions": sessions().len(),
    });

    let audit_stats = audit::audit().stats();

    Response::json_ok(&json!({
        "name": crate::mcp::protocol::SERVER_NAME,
        "version": crate::mcp::protocol::SERVER_VERSION,
        "protocolVersion": crate::mcp::protocol::PROTOCOL_VERSION,
        "supportedProtocolVersions": crate::mcp::protocol::SUPPORTED_PROTOCOL_VERSIONS,
        "transports": {
            "sse": format!("{}/sse", DEFAULT_MCP_PREFIX),
            "message": format!("{}/message", DEFAULT_MCP_PREFIX),
            "streamableHttp": DEFAULT_MCP_PREFIX,
            "stdio": "gritshield::mcp::stdio::serve_stdio()"
        },
        "counts": counts,
        "audit": audit_stats,
        "authenticationRequired": auth::require_authentication(),
    }))
}

/// Reject a JSON-RPC error that escaped into the transport layer.
pub fn internal_error(context: &str, error: McpError) -> Response {
    audit::audit().record(
        "transport/internal",
        context,
        "system",
        None,
        "0.0.0.0",
        None,
        json!({}),
        outcome::FAILED,
        Some(error.to_string()),
        0,
    );

    Response::json_internal_error(&json!({ "error": error.to_string() }))
}

/// Register every MCP HTTP route on the router trie.
pub(crate) fn register(prefix: &str) -> Vec<(&'static str, crate::http::HttpMethod, String)> {
    vec![
        ("sse", crate::http::HttpMethod::GET, format!("{}/sse", prefix)),
        (
            "message",
            crate::http::HttpMethod::POST,
            format!("{}/message", prefix),
        ),
        (
            "streamable",
            crate::http::HttpMethod::POST,
            prefix.to_string(),
        ),
        (
            "terminate",
            crate::http::HttpMethod::DELETE,
            prefix.to_string(),
        ),
        (
            "info",
            crate::http::HttpMethod::GET,
            prefix.to_string(),
        ),
    ]
}

/// The set of MCP HTTP routes, for logging and tests.
pub fn route_table(prefix: &str) -> Vec<(&'static str, crate::http::HttpMethod, String)> {
    register(prefix)
}

/// Handle an already-parsed dispatch, for transports that are not HTTP.
pub async fn respond(dispatch: McpDispatch) -> Option<JsonRpcResponse> {
    dispatch.response
}