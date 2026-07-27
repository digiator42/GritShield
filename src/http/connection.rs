use crate::{
    core::env::get_env,
    debug, error,
    http::{request::Request, response::Response},
    middleware::MiddlewareResult,
    routing::{
        engine::{RequestContext, Router, RoutingResult, GLOBAL_FALLBACK},
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
    // Increment active connections once per TCP lifecycle
    router
        .telemetry
        .active_connections
        .fetch_add(1, Ordering::Relaxed);

    // Fetch secret key once per connection rather than every single request
    let secret_key = get_env("JWT_SECRET", "fallback_secure_key_string");

    // Keep TCP connection open across multiple HTTP requests (HTTP Keep-Alive)
    loop {
        // Parse raw request wire components
        let req = match Request::parse(&mut stream).await {
            Ok(parsed_req) => parsed_req,
            Err(e) => {
                // If the stream closed cleanly or hit EOF, terminate connection loop
                let err_msg = e.to_string();
                if err_msg.contains("EOF")
                    || err_msg.contains("Closed")
                    || err_msg.contains("reset")
                {
                    break;
                }
                warn!("{} {}", "Security Warning:".red().bold(), e);
                let err_res = Response::new(400, Sanitizer::trust("<h1>Bad Request</h1>"));
                let (bytes, mime) = err_res.resolve();
                let _ = stream.write_all(&err_res.to_bytes(&bytes, &mime)).await;
                break;
            }
        };

        let start_time = std::time::Instant::now();

        // Determine if connection should stay open after response
        let keep_alive = req
            .headers
            .get("connection")
            .or_else(|| req.headers.get("Connection"))
            .map_or(true, |v| v.to_lowercase() != "close");

        let req_clone = req.clone();

        // Match the route early to extract dynamic params for middleware use
        let params = match router.match_route(&req.method, &req.path) {
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
            secret_key.clone(),
        )));

        let telemetry = router.telemetry.clone();
        let event_bus = router.event_bus.clone();
        let job_queue = router.job_queue.clone();

        let mut ctx = RequestContext {
            params,
            telemetry,
            event_bus,
            job_queue,
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
            role_inheritance: Arc::new(router.role_inheritance.clone()),
        };

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

                // router.log_lifecycle(&ctx, err_res.status, duration);
                router.run_after_hooks(ctx, err_res.status, duration);

                if !keep_alive {
                    break;
                }
                continue;
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
                debug!("[CORE ENGINE] Upgrading socket connection path to WebSocket stream.");

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

                    if let Err(e) = stream.write_all(handshake_response.as_bytes()).await {
                        error!("[WS ERROR] Failed to send handshake response: {:?}", e);
                        break;
                    }

                    let ws_stream = tokio_tungstenite::WebSocketStream::from_raw_socket(
                        stream,
                        tokio_tungstenite::tungstenite::protocol::Role::Server,
                        None,
                    )
                    .await;

                    tokio::spawn(async move {
                        debug!("[ACC TELEMETRY] Live Monitoring Operator Connected!");
                        ws_handler(ws_stream, ctx).await;
                    });

                    // WS takes over socket ownership
                    router
                        .telemetry
                        .active_connections
                        .fetch_sub(1, Ordering::Relaxed);
                    return;
                } else {
                    error!("[WS ERROR] Missing Sec-WebSocket-Key header.");
                }
            }
            warn!(
                "[WS WARN] WebSocket upgrade requested for unregistered path: {}",
                ctx.req.path
            );
        }

        let error_handler_ptr = router.global_error_handler.handler;
        let router_clone = router.clone();
        let ctx_clone = ctx.clone();

        // Route Execution Future
        let response_future = async move {
            match router_clone.match_route(&ctx.req.method, &ctx.req.path) {
                RoutingResult::Found(handler, required_role, _) => {
                    // AUTOMATED ACCESS CONTROL MATRIX (RBAC Guard)
                    // Look up if this matching URL route path has an explicit role requirement attached
                    if let Some(required_role) = required_role {
                        if !ctx.has_role(required_role) {
                            error!(
                                "[RBAC SHIELD] Blocked Unauthorized Access attempt to {} | Missing operational clearance: {}",
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

                    let mut response: Response = handler.call(ctx.clone()).await;

                    for (key, value) in ctx.headers.iter() {
                        if !response.headers.iter().any(|(k, _)| k == key) {
                            response.headers.push((key.clone(), value.clone()));
                        }
                    }

                    router_clone.log_lifecycle(&ctx, response.status, start_time.elapsed());
                    router_clone.run_after_hooks(ctx, response.status, start_time.elapsed());

                    response
                }
                RoutingResult::NotFound => {
                    let fallback_opt = if let Ok(guard) = GLOBAL_FALLBACK.lock() {
                        guard.clone()
                    } else {
                        None
                    };

                    if let Some(custom_fallback) = fallback_opt {
                        custom_fallback(ctx).await
                    } else {
                        if let Some(err_handler) = error_handler_ptr {
                            err_handler(ctx, ShieldError::NotFound).await
                        } else {
                            Response::new(404, Sanitizer::trust("<h1>404 Not Found</h1>"))
                        }
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
                let panic_msg = if let Some(s) = panic_payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = panic_payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "Unknown framework thread panic occurred.".to_string()
                };

                error!("[PANIC INFRASTRUCTURE SHIELD] Caught: {}", panic_msg);

                if let Some(custom_err_hook) = error_handler_ptr {
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

        let duration = start_time.elapsed();

        router
            .telemetry
            .record_request(&req_clone.path, response.status, duration);

        if let Ok(locked_jar) = jar.lock() {
            response = locked_jar.clone().commit(response);
        }

        let (bytes, mime) = response.resolve();

        // Write back to stream; break loop if write fails (client disconnected)
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

    // Decrement active connections upon socket termination
    router
        .telemetry
        .active_connections
        .fetch_sub(1, Ordering::Relaxed);
}
