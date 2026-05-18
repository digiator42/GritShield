use crate::{
    protocol::{request::Request, response::Response},
    routing::trie::{RequestContext, Router, RoutingResult},
    security::{
        cookies::CookieJar, errors::FrameworkError, middleware::MiddlewareResult, xss::Sanitizer,
    },
};
use colored::Colorize;
use futures::future::{self, BoxFuture, FutureExt};
use std::panic::AssertUnwindSafe;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

pub async fn handle_connection(mut stream: TcpStream, router: Arc<Router>) {
    let start_time = std::time::Instant::now();

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

    // Pre-build our core Context Manager object
    let routing_result = router.match_route(&req.method, &req.path);

    let params = match &routing_result {
        RoutingResult::Found(_, dynamic_params) => dynamic_params.clone(),
        _ => HashMap::new(),
    };

    let form = req.parse_form_body();

    // We look up the raw "cookie" header case-insensitively or lowercase based on your parser
    let cookie_header = req
        .headers
        .get("cookie")
        .or_else(|| req.headers.get("Cookie"));
    let secret_key = crate::core::env::get_env("JWT_SECRET", "fallback_secure_key_string");
    let jar = Arc::new(Mutex::new(CookieJar::new(cookie_header, secret_key)));

    let mut ctx = RequestContext {
        params,
        headers: req.headers.clone(),
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
    };

    // Process Middleware Stack sequentially
    match router.run_middlewares(&mut ctx) {
        MiddlewareResult::Next(maybe_state) => {
            // Unpack the final accumulated values directly into your request context
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
            // err_res = ctx.cookies.clone().commit(err_res);

            let (bytes, mime) = err_res.resolve();
            let _ = stream.write_all(&err_res.to_bytes(&bytes, &mime)).await;

            if router.use_logger {
                router.log_lifecycle(&ctx, err_res.status, start_time.elapsed());
            }

            router.run_after_hooks(ctx, err_res.status, start_time.elapsed());

            return;
        }
    }

    let error_handler_ptr = router.global_error_handler.handler;
    // Route Execution
    let response_future = async move {
        match routing_result {
            RoutingResult::Found(handler, _) => {
                // Process handler with our loaded and mutated context manager
                let response: Response = handler(ctx.clone()).await;

                if router.use_logger {
                    router.log_lifecycle(&ctx, response.status, start_time.elapsed());
                }

                router.run_after_hooks(ctx.clone(), response.status, start_time.elapsed());

                response
            }
            RoutingResult::NotFound => Response::new(404, Sanitizer::trust("<h1>404</h1>")),
            RoutingResult::MethodNotAllowed => Response::new(405, Sanitizer::trust("<h1>405</h1>")),
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
                let fallback_ctx = RequestContext::new();
                custom_err_hook(fallback_ctx, FrameworkError::Panic(panic_msg)).await
            } else {
                Response::new(500, Sanitizer::trust("<h1>500 Internal Server Error</h1>"))
            }
        }
    };

    // This drains all staged/mutated cookies out of the jar right into the response header queue
    if let Ok(locked_jar) = jar.lock() {
        // Moves staging cookies out of the shared pointer into the real response headers!
        response = locked_jar.clone().commit(response);
    }

    // Send output back over socket wire
    let (bytes, mime) = response.resolve();
    let _ = stream.write_all(&response.to_bytes(&bytes, &mime)).await;
}
