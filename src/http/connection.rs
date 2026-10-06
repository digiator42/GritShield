use crate::{
    database::repository::transaction::{CURRENT_EVENT_BUS, CURRENT_JOB_QUEUE},
    error,
    http::{
        request::HttpMethod, request::Request, response::Response, sse, FormData, ResponseBody,
    },
    middleware::MiddlewareResult,
    routing::{
        engine::{RequestContext, Router, RoutingResult},
        websocket::{SUPPORTED_WS_VERSION, has_ws_route, match_ws_route},
    },
    security::{cookies::CookieJar, errors::ShieldError, xss::Sanitizer},
    warn,
};
use futures::future::FutureExt;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

/// The single funnel every buffered response passes through before being
/// written, so middleware behaves identically on success, rejection, 404/405
/// and handler panics:
///
/// 1. `merge_middleware_headers` — headers middleware put in `ctx.headers`
/// 2. cookie commit — so `on_response` sees the final `Set-Cookie`
/// 3. `Middleware::on_response`, registration order **reversed**, for the
///    `ran` middlewares that actually executed
/// 4. `log_lifecycle` → `AfterRequestHook`s → telemetry — after `on_response`,
///    so a status rewritten there is what gets logged and counted
///
/// `ran` is the count from `Router::run_middlewares_counted`; `ctx` is the
/// pre-handler snapshot on the normal path (`hook_ctx`) and the live context
/// on early exits where the handler never consumed it.
async fn finalize_response(
    router: &Router,
    ctx: &RequestContext,
    response: Response,
    request_header_names: &std::collections::HashSet<String>,
    jar: &Arc<Mutex<CookieJar>>,
    duration: std::time::Duration,
    ran: usize,
) -> Response {
    let mut response = response;

    response.merge_middleware_headers(&ctx.headers, request_header_names);

    if let Ok(locked_jar) = jar.lock() {
        response = locked_jar.clone().commit(response);
    }

    router.run_on_response(ctx, &mut response, ran).await;

    router.log_lifecycle(ctx, response.status, duration).await;
    router.run_after_hooks(ctx, response.status, duration).await;
    router
        .telemetry
        .record_request(&ctx.req.path, response.status, duration);

    response
}

pub async fn handle_connection(mut stream: TcpStream, peer_addr: SocketAddr, router: Arc<Router>) {
    CURRENT_EVENT_BUS.scope(router.event_bus.clone(), {
        CURRENT_JOB_QUEUE.scope(router.job_queue.clone(), async move {
            router.telemetry.open_connection();

            let mut read_buf = vec![0u8; 16 * 1024];
            let _ = stream.set_nodelay(true);

            loop {
                let req = match Request::parse(&mut stream, &mut read_buf).await {
                    Ok(parsed_req) => parsed_req,
                    Err(e) => {
                        let err_msg = e.to_string();
                        if err_msg.contains("EOF")
                            || err_msg.contains("Closed")
                            || err_msg.contains("reset")
                        {
                            break;
                        }
                        warn!("{}", e);
                        let err_res = Response::new(400, Sanitizer::trust("<h1>Bad Request</h1>"));
                        let (bytes, mime) = err_res.resolve();
                        let _ = stream.write_all(&err_res.to_bytes(&bytes, &mime)).await;
                        break;
                    }
                };

                let start_time = std::time::Instant::now();

                let keep_alive = req
                    .header("connection")
                    // header values are Vec<String>, so check if any value equals "close" (case-insensitive)
                    .map_or(true, |v| !v.iter().any(|s| s.eq_ignore_ascii_case("close")));

                let routing_result = router.match_route(&req.method, &req.path);

                let params = match &routing_result {
                    RoutingResult::Found(_, _, dynamic_params) => dynamic_params.clone(),
                    _ => HashMap::new(),
                };

                // Lazy Form Parsing: Only execute form parsing for HTTP methods that accept payloads
                let form = match req.method {
                    HttpMethod::GET | HttpMethod::HEAD | HttpMethod::OPTIONS => FormData::new(),
                    _ => req.parse_form_body(),
                };

                let cookie_header = req.header("cookie").and_then(|v| v.get(0));

                let jar = Arc::new(Mutex::new(CookieJar::new(
                    cookie_header,
                    router.secret_key.clone(),
                )));

                // Header names that arrived with the request. Request headers are
                // lowercased during parsing (`Request::parse`), so normalising
                // here makes the comparison below case-insensitive on both sides.
                //
                // This snapshot is what separates a header the *client* sent from
                // one a *middleware* added while handling the request. Only the
                // latter is eligible to be copied onto the response.
                let request_header_names: std::collections::HashSet<String> =
                    req.headers.keys().map(|k| k.to_lowercase()).collect();

                let mut ctx = RequestContext {
                    params,
                    telemetry: router.telemetry.clone(),
                    event_bus: router.event_bus.clone(),
                    job_queue: router.job_queue.clone(),
                    headers: req.headers.clone(),
                    peer_addr,
                    claims: None,
                    query: req.query.clone(),
                    session: None,
                    form,
                    db: router.db.clone(),
                    raw_body: req.body.clone(),
                    content_type: req
                        .header("content-type")
                        .and_then(|v| v.first().cloned()),
                    req,
                    cookies: jar.clone(),
                    start_time,
                    role_inheritance: router.role_inheritance.clone(),
                };

                let (ran, middleware_result) = router.run_middlewares_counted(&mut ctx).await;
                match middleware_result {
                    MiddlewareResult::Next(maybe_state) => {
                        if let Some(state) = maybe_state {
                            if state.session.is_some() {
                                ctx.session = state.session;
                            }
                            if state.claims.is_some() {
                                ctx.claims = state.claims;
                            }
                        }
                    }
                    MiddlewareResult::Error(err_res) => {
                        let duration = start_time.elapsed();
                        let err_res = finalize_response(
                            &router,
                            &ctx,
                            err_res,
                            &request_header_names,
                            &jar,
                            duration,
                            ran,
                        )
                        .await;

                        let (bytes, mime) = err_res.resolve();
                        if stream
                            .write_all(&err_res.to_bytes(&bytes, &mime))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        if !keep_alive {
                            break;
                        }
                        continue;
                    }
                }

                let wants_upgrade = ctx
                    .req
                    .headers
                    .get("upgrade")
                    .map_or(false, |v| v.iter().any(|val| val.eq_ignore_ascii_case("websocket")));

                if wants_upgrade {
                    let offered_subprotocols: Vec<String> = ctx
                        .req
                        .headers
                        .get("sec-websocket-protocol")
                        .map(|values| {
                            values
                                .iter()
                                .flat_map(|value| value.split(','))
                                .map(|token| token.trim().to_string())
                                .filter(|token| !token.is_empty())
                                .collect()
                        })
                        .unwrap_or_default();

                    let ws_match = match_ws_route(&ctx.req.path, &offered_subprotocols);

                    if let Some(matched) = ws_match {
                        // RFC 6455 §4.2.1: a client speaking anything other than
                        // version 13 gets 426, not a 101 it cannot speak.
                        let version_ok = ctx
                            .req
                            .headers
                            .get("sec-websocket-version")
                            .and_then(|values| values.first())
                            .map(|value| value.trim() == SUPPORTED_WS_VERSION)
                            .unwrap_or(false);

                        let key = ctx
                            .req
                            .headers
                            .get("sec-websocket-key")
                            .and_then(|values| values.first())
                            .cloned();

                        if let (true, Some(key)) = (version_ok, key) {
                            let accept_hash =
                                tokio_tungstenite::tungstenite::handshake::derive_accept_key(
                                    key.as_bytes(),
                                );

                            let mut handshake_response = format!(
                                "HTTP/1.1 101 Switching Protocols\r\n\
                                Upgrade: websocket\r\n\
                                Connection: Upgrade\r\n\
                                Sec-WebSocket-Accept: {}\r\n",
                                accept_hash
                            );

                            if let Some(protocol) = matched.subprotocol.as_deref() {
                                handshake_response.push_str(&format!(
                                    "Sec-WebSocket-Protocol: {}\r\n",
                                    protocol
                                ));
                            }
                            handshake_response.push_str("\r\n");

                            if stream
                                .write_all(handshake_response.as_bytes())
                                .await
                                .is_err()
                            {
                                break;
                            }

                            let ws_stream = tokio_tungstenite::WebSocketStream::from_raw_socket(
                                stream,
                                tokio_tungstenite::tungstenite::protocol::Role::Server,
                                None,
                            )
                            .await;

                            let handler = matched.handler;
                            ctx.params.extend(matched.params);

                            let telemetry = router.telemetry.clone();
                            tokio::spawn(async move {
                                handler(ws_stream, ctx).await;
                                // The connection is only really over once the
                                // WebSocket session ends, so the gauge is
                                // released here rather than at upgrade time.
                                telemetry.close_connection();
                            });

                            return;
                        }

                        let (status, body) = if version_ok {
                            warn!("{}", "WebSocket upgrade rejected: missing Sec-WebSocket-Key");
                            (400, "<h1>Bad Request</h1>")
                        } else {
                            warn!(
                                "{}",
                                format!(
                                    "WebSocket upgrade rejected: unsupported Sec-WebSocket-Version (expected {})",
                                    SUPPORTED_WS_VERSION
                                )
                            );
                            (426, "<h1>Upgrade Required</h1>")
                        };

                        let mut reject_res = Response::new(status, Sanitizer::trust(body));
                        reject_res
                            .headers
                            .push(("Sec-WebSocket-Version".to_string(), SUPPORTED_WS_VERSION.to_string()));
                        reject_res.headers.push((
                            "Connection".to_string(),
                            "Upgrade".to_string(),
                        ));
                        let (bytes, mime) = reject_res.resolve();
                        let _ = stream.write_all(&reject_res.to_bytes(&bytes, &mime)).await;
                        break;
                    } else {
                        warn!(
                            "{}",
                            format!("WebSocket upgrade requested for unregistered path: {}", ctx.req.path)
                        );
                    }
                } else if has_ws_route(&ctx.req.path) {
                    // A plain request to a WebSocket endpoint should say so,
                    // rather than looking like a missing route.
                    let upgrade_res = Response::new(
                        426,
                        Sanitizer::trust("<h1>Upgrade Required</h1>"),
                    );
                    let (bytes, mime) = upgrade_res.resolve();
                    let _ = stream.write_all(&upgrade_res.to_bytes(&bytes, &mime)).await;
                    break;
                }

                let error_handler_ptr = router.global_error_handler.handler;
                let hook_ctx = ctx.clone();

                let response_future = async move {
                    match routing_result {
                        RoutingResult::Found(handler, required_role, _) => {
                            if let Some(required_role) = required_role {
                                if !ctx.has_role(required_role) {
                                    return Response::forbidden(&HashMap::from([(
                                        "error",
                                        format!(
                                            "Access Denied: Missing required operational role clearance '{}'.",
                                            required_role
                                        ),
                                    )]));
                                }
                            }

                            // Middleware-added headers are merged onto the
                            // response in `finalize_response`, alongside
                            // `on_response`, so they also reach 404/405/panic
                            // and rejection responses. `hook_ctx` holds the
                            // same post-middleware snapshot `ctx` had here.
                            handler.call(ctx).await
                        }
                        RoutingResult::NotFound => {
                            if let Some(err_handler) = error_handler_ptr {
                                err_handler(ctx, ShieldError::NotFound).await
                            } else {
                                Response::new(404, Sanitizer::trust("<h1>404 Not Found</h1>"))
                            }
                        }
                        RoutingResult::MethodNotAllowed => {
                            if let Some(err_handler) = error_handler_ptr {
                                err_handler(ctx, ShieldError::MethodNotAllowed).await
                            } else {
                                Response::new(405, Sanitizer::trust("<h1>405 Method Not Allowed</h1>"))
                            }
                        }
                    }
                };

                let response = match std::panic::AssertUnwindSafe(response_future)
                    .catch_unwind()
                    .await
                {
                    Ok(normal_response) => normal_response,
                    Err(panic_payload) => {
                        let panic_msg = panic_payload
                            .downcast_ref::<&str>()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| {
                                panic_payload
                                    .downcast_ref::<String>()
                                    .cloned()
                                    .unwrap_or_else(|| "Unknown framework panic occurred.".to_string())
                            });

                        error!("[PANIC INFRASTRUCTURE SHIELD] Caught: {}", panic_msg);

                        Response::new(500, Sanitizer::trust("<h1>500 Internal Server Error</h1>"))
                    }
                };

                let duration = start_time.elapsed();

                let response = finalize_response(
                    &router,
                    &hook_ctx,
                    response,
                    &request_header_names,
                    &jar,
                    duration,
                    router.middlewares.len(),
                )
                .await;

                // A streaming body (SSE) is pumped incrementally and holds the
                // socket open for its whole lifetime, so it bypasses both the
                // buffered `resolve()` path and the keep-alive loop below.
                if response.is_streaming() {
                    // Serialize the head while `response` is still whole, then
                    // take ownership of the stream body.
                    let head = response.to_stream_head_bytes("text/event-stream; charset=utf-8");

                    if let ResponseBody::Stream(sse) = response.body {
                        let keep_alive = sse.keep_alive();

                        if stream.write_all(&head).await.is_err() {
                            break;
                        }

                        // `response` is consumed here, so the only remaining
                        // sender is whatever long-lived producer owns the
                        // session. When `pump` returns, that producer has
                        // genuinely gone away.
                        if sse::pump(&mut stream, sse, keep_alive).await.is_err() {
                            break;
                        }
                    }

                    router.telemetry.close_connection();
                    return;
                }

                let (bytes, mime) = response.resolve();

                if stream
                    .write_all(&response.to_bytes(&bytes, &mime))
                    .await
                    .is_err()
                {
                    break;
                }

                if !keep_alive {
                    break;
                }
            }

            router.telemetry.close_connection();
        })
    }).await;
}