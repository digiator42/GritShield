use crate::{
    protocol::{request::Request, response::Response},
    routing::{
        trie::{IntoHandler, RequestContext, Router, RoutingResult},
        websocket::WS_REGISTRY,
    },
    security::{
        cookies::CookieJar, errors::ShieldError, middleware::MiddlewareResult, xss::Sanitizer,
    },
};
use colored::Colorize;
use futures::future::{self, BoxFuture, FutureExt};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use std::{net::SocketAddr, panic::AssertUnwindSafe, sync::atomic::Ordering};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

pub async fn handle_connection(mut stream: TcpStream, peer_addr: SocketAddr, router: Arc<Router>) {
    let start_time = std::time::Instant::now();

    router
        .telemetry
        .active_connections
        .fetch_add(1, Ordering::SeqCst);

    // Parse raw request wire components
    let req = match Request::parse(&mut stream).await {
        Ok(parsed_req) => parsed_req,
        Err(e) => {
            eprintln!("{} {}", "Security Warning:".red().bold(), e);
            let err_res = Response::new(400, Sanitizer::trust("<h1>Bad Request</h1>"));
            let (bytes, mime) = err_res.resolve();
            let _ = stream.write_all(&err_res.to_bytes(&bytes, &mime)).await;
            return;
        }
    };

    // Match the route early to extract dynamic params for middleware use, even if the final handler isn't found
    let params = match router.match_route(&req.method, &req.path) {
        RoutingResult::Found(_, dynamic_params) => dynamic_params.clone(),
        _ => HashMap::new(),
    };

    let form = req.parse_form_body();
    let cookie_header = req
        .headers
        .get("cookie")
        .or_else(|| req.headers.get("Cookie"));
    let secret_key = crate::core::env::get_env("JWT_SECRET", "fallback_secure_key_string");
    let jar = Arc::new(Mutex::new(CookieJar::new(cookie_header, secret_key)));

    let telemetry = router.telemetry.clone();

    let mut ctx = RequestContext {
        params,
        telemetry,
        headers: req.headers.clone(),
        peer_addr: peer_addr,
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
        role_inheritance: Arc::new(router.role_inheritance.clone()),
    };

    let ctx_clone = ctx.clone();

    // Process Middleware Stack sequentially
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
        MiddlewareResult::Error(err_res) => {
            let (bytes, mime) = err_res.resolve();
            let _ = stream.write_all(&err_res.to_bytes(&bytes, &mime)).await;

            if router.use_logger {
                router.log_lifecycle(&ctx, err_res.status, start_time.elapsed());
            }
            router.run_after_hooks(ctx, err_res.status, start_time.elapsed());
            return;
        }
    }

    let is_ws_request = ctx
        .req
        .headers
        .get("upgrade")
        .map_or(false, |v| v == "websocket");

    if is_ws_request {
        let target_ws_handler = {
            let ws_routes = WS_REGISTRY.lock().unwrap();
            ws_routes.get(&ctx.req.path).cloned()
        };

        if let Some(ws_handler) = target_ws_handler {
            println!("[CORE ENGINE] Upgrading socket connection path to WebSocket stream.");

            // Manually build the WebSocket Handshake Accept response using the parsed headers
            if let Some(key) = ctx.req.headers.get("sec-websocket-key") {
                // Generate the cryptographic handshake accept token
                let accept_hash =
                    tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());

                // Formulate a proper 101 Switching Protocols HTTP response frame
                let handshake_response = format!(
                    "HTTP/1.1 101 Switching Protocols\r\n\
                    Upgrade: websocket\r\n\
                    Connection: Upgrade\r\n\
                    Sec-WebSocket-Accept: {}\r\n\r\n",
                    accept_hash
                );

                // Write the handshake directly to the open TCP socket
                if let Err(e) = stream.write_all(handshake_response.as_bytes()).await {
                    eprintln!("[WS ERROR] Failed to send handshake response: {:?}", e);
                    return;
                }

                // Convert the raw TCP socket into a WebSocketStream directly, bypassing tungstenite's parser loop
                let ws_stream = tokio_tungstenite::WebSocketStream::from_raw_socket(
                    stream,
                    tokio_tungstenite::tungstenite::protocol::Role::Server,
                    None,
                )
                .await;

                // Hand off the completely active stream to telemetry worker loop
                tokio::spawn(async move {
                    println!("[ACC TELEMETRY] Live Monitoring Operator Connected!");
                    ws_handler(ws_stream, ctx).await;
                });

                return;
            } else {
                eprintln!("[WS ERROR] Missing Sec-WebSocket-Key header.");
            }
        }
        println!(
            "[WS WARN] WebSocket upgrade requested for unregistered path: {}",
            ctx.req.path
        );
    }

    let error_handler_ptr = router.global_error_handler.handler;

    // Clone our Arc handle for the execution block
    let router_clone = router.clone();

    // Route Execution Future
    let response_future = async move {
        // router_clone is ultra short-lived and never crosses an outer function boundary.
        match router_clone.match_route(&ctx.req.method, &ctx.req.path) {
            RoutingResult::Found(handler, _) => {
                // AUTOMATED ACCESS CONTROL MATRIX (RBAC Guard)
                // Look up if this matching URL route path has an explicit role requirement attached
                if let Some(required_role) = router_clone.role_registry.get(&ctx.req.path) {
                    // Evaluates Fixed Engine rules FIRST, falling back to Dynamic tree links seamlessly
                    if !ctx.has_role(required_role) {
                        println!(
                            "\x1b[31m[RBAC SHIELD] Blocked Unauthorized Access attempt to {} | Missing operational clearance: {}\x1b[0m",
                            ctx.req.path, required_role
                        );

                        return Response::forbidden(&HashMap::from([(
                            "error",
                            format!(
                                "Access Denied: Missing required operational role clearance '{}'.",
                                required_role
                            ),
                        )]));
                    }
                }

                let response: Response = handler.call(ctx.clone()).await;

                if router_clone.use_logger {
                    router_clone.log_lifecycle(&ctx, response.status, start_time.elapsed());
                }
                router_clone.run_after_hooks(ctx.clone(), response.status, start_time.elapsed());

                response
            }
            RoutingResult::NotFound => {
                // Look up the global macro-registered fallback state
                let fallback_opt = if let Ok(guard) = crate::routing::trie::GLOBAL_FALLBACK.lock() {
                    guard.clone()
                } else {
                    None
                };

                if let Some(custom_fallback) = fallback_opt {
                    // Call the function pointer and .await the returned async execution stream!
                    custom_fallback(ctx).await
                } else {
                    // Trigger the global error handler for 404s to keep styling consistent
                    if let Some(err_handler) = error_handler_ptr {
                        err_handler(ctx, ShieldError::NotFound).await
                    } else {
                        Response::new(404, Sanitizer::trust("<h1>404 Not Found</h1>"))
                    }
                }
            }
            RoutingResult::MethodNotAllowed => {
                // Trigger the global error handler for 405s to keep styling consistent
                if let Some(err_handler) = error_handler_ptr {
                    err_handler(ctx, ShieldError::MethodNotAllowed).await
                } else {
                    Response::new(405, Sanitizer::trust("<h1>405 Method Not Allowed</h1>"))
                }
            }
        }
    };

    // This ensures catch_unwind monitors the execution of the async block
    let mut response = match std::panic::AssertUnwindSafe(response_future)
        .catch_unwind()
        .await
    {
        Ok(normal_response) => normal_response,
        Err(panic_payload) => {
            let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown framework thread panic occurred.".to_string()
            };

            eprintln!("[PANIC INFRASTRUCTURE SHIELD] Caught: {}", panic_msg);

            if let Some(custom_err_hook) = error_handler_ptr {
                // let fallback_ctx = RequestContext::new();
                // Updated to use the struct variant syntax
                custom_err_hook(
                    ctx_clone,
                    ShieldError::Panic {
                        message: panic_msg,
                        backtrace: std::backtrace::Backtrace::capture(),
                    },
                )
                .await
            } else {
                Response::new(500, Sanitizer::trust("<h1>500 Internal Server Error</h1>"))
            }
        }
    };

    if let Ok(locked_jar) = jar.lock() {
        response = locked_jar.clone().commit(response);
    }

    router
        .telemetry
        .active_connections
        .fetch_sub(1, Ordering::SeqCst);

    let (bytes, mime) = response.resolve();
    let _ = stream.write_all(&response.to_bytes(&bytes, &mime)).await;
}
