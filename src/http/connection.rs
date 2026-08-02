use crate::{
    database::repository::transaction::{CURRENT_EVENT_BUS, CURRENT_JOB_QUEUE},
    error,
    http::{request::Request, response::Response},
    middleware::MiddlewareResult,
    routing::{
        engine::{RequestContext, Router, RoutingResult},
        websocket::WS_REGISTRY,
    },
    security::{cookies::CookieJar, errors::ShieldError, xss::Sanitizer},
    warn,
};
use colored::Colorize;
use futures::future::FutureExt;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use std::{net::SocketAddr, sync::atomic::Ordering};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

pub async fn handle_connection(mut stream: TcpStream, peer_addr: SocketAddr, router: Arc<Router>) {
    CURRENT_EVENT_BUS.scope(router.event_bus.clone(), {
        CURRENT_JOB_QUEUE.scope(router.job_queue.clone(), async move {
            router
                .telemetry
                .active_connections
                .fetch_add(1, Ordering::Relaxed);

            // One reusable buffer per connection, not per request
            let mut read_buf = vec![0u8; 16 * 1024];

            // Nagle's algorithm batches small writes waiting for an ACK, which is exactly
            // what produces the occasional multi-hundred-ms tail latency spike under
            // concurrent load on a raw-socket server like this one.
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
                    .headers
                    .get("connection")
                    .or_else(|| req.headers.get("Connection"))
                    .map_or(true, |v| v.to_lowercase() != "close");

                // Single route match pass
                let routing_result = router.match_route(&req.method, &req.path);

                let params = match &routing_result {
                    RoutingResult::Found(_, _, dynamic_params) => dynamic_params.clone(),
                    _ => HashMap::new(),
                };

                let form = req.parse_form_body();
                let cookie_header = req
                    .headers
                    .get("cookie")
                    .or_else(|| req.headers.get("Cookie"));

                let jar = Arc::new(Mutex::new(CookieJar::new(
                    cookie_header,
                    router.secret_key.clone(),
                )));

                // Construct RequestContext without duplicating req inner fields
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
                    content_type: req.headers.get("content-type").cloned(),
                    req,
                    cookies: jar.clone(),
                    start_time,
                    role_inheritance: router.role_inheritance.clone(),
                };

                // Process Middleware Stack
                match router.run_middlewares(&mut ctx) {
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
                    MiddlewareResult::Error(mut err_res) => {
                        let duration = start_time.elapsed();
                        ctx.telemetry
                            .record_request(&ctx.req.path, err_res.status, duration);

                        if let Ok(locked_jar) = jar.lock() {
                            err_res = locked_jar.clone().commit(err_res);
                        }

                        let (bytes, mime) = err_res.resolve();
                        if stream
                            .write_all(&err_res.to_bytes(&bytes, &mime))
                            .await
                            .is_err()
                        {
                            break;
                        }

                        router.run_after_hooks(&ctx, err_res.status, duration).await;

                        if !keep_alive {
                            break;
                        }
                        continue;
                    }
                }

                // WebSocket handling check
                if ctx
                    .req
                    .headers
                    .get("upgrade")
                    .map_or(false, |v| v == "websocket")
                {
                    let target_ws_handler = {
                        let ws_routes = WS_REGISTRY.lock().unwrap();
                        ws_routes.get(&ctx.req.path).cloned()
                    };

                    if let Some(ws_handler) = target_ws_handler {
                        if let Some(key) = ctx.req.headers.get("sec-websocket-key") {
                            let accept_hash = tokio_tungstenite::tungstenite::handshake::derive_accept_key(
                                key.as_bytes(),
                            );

                            let handshake_response = format!(
                                "HTTP/1.1 101 Switching Protocols\r\n\
                                Upgrade: websocket\r\n\
                                Connection: Upgrade\r\n\
                                Sec-WebSocket-Accept: {}\r\n\r\n",
                                accept_hash
                            );

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

                            tokio::spawn(async move {
                                ws_handler(ws_stream, ctx).await;
                            });

                            router
                                .telemetry
                                .active_connections
                                .fetch_sub(1, Ordering::Relaxed);
                            return;
                        }
                    }
                }

                let error_handler_ptr = router.global_error_handler.handler;

                // Save request path reference before passing ctx into async task
                let req_path = ctx.req.path.clone();

                let hook_ctx = ctx.clone();

                // Route Execution
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

                            let headers = ctx.headers.clone();

                            let mut response: Response = handler.call(ctx).await;

                            for (key, value) in headers.iter() {
                                if !response.headers.iter().any(|(k, _)| k == key) {
                                    response.headers.push((key.clone(), value.clone()));
                                }
                            }

                            response
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

                let mut response = match std::panic::AssertUnwindSafe(response_future)
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

                router.log_lifecycle(&hook_ctx, response.status, duration);
                router.run_after_hooks(&hook_ctx, response.status, duration).await;

                router
                    .telemetry
                    .record_request(&req_path, response.status, duration);

                if let Ok(locked_jar) = jar.lock() {
                    response = locked_jar.clone().commit(response);
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

            router
                .telemetry
                .active_connections
                .fetch_sub(1, Ordering::Relaxed);
        })
    }).await;
}
