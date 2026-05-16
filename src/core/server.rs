use colored::*;

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::time::{Duration, sleep};

use crate::core::logger::log_request_summary;
use crate::protocol::request::Request;
use crate::protocol::response::Response;
use crate::routing::trie::{RequestContext, Router, RoutingResult};
use crate::security::middleware::MiddlewareResult;
use crate::security::xss::Sanitizer;
use crate::utils::reloader::HotReloader;

pub async fn run_server(host: &str, port: &str, router: Router, use_reloader: bool) {
    if use_reloader {
        HotReloader::start();

        if std::env::var("RUNNING_UNDER_RELOADER").is_err() {
            std::process::exit(0);
        }
    }

    let listener = TcpListener::bind(format!("{}:{}", host, port))
        .await
        .unwrap();

    println!(
        "{} {}:{}",
        "[GRITSHIELD] Server Online at".green().bold(),
        host,
        port
    );

    let router = Arc::new(router);

    // graceful shutdown broadcaster
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // active connection tracker
    let active_connections = Arc::new(AtomicUsize::new(0));

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!(
                    "{}",
                    "[GRITSHIELD] Shutdown signal received"
                        .yellow()
                        .bold()
                );

                // notify all tasks
                let _ = shutdown_tx.send(());

                break;
            }

            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _)) => {

                        active_connections
                            .fetch_add(1, Ordering::SeqCst);

                        let router =
                            Arc::clone(&router);

                        let active_connections =
                            Arc::clone(&active_connections);

                        let mut shutdown_rx =
                            shutdown_tx.subscribe();

                        tokio::spawn(async move {

                            tokio::select! {

                                _ = shutdown_rx.recv() => {
                                    // server shutting down
                                }

                                _ = handle_connection(
                                    stream,
                                    router
                                ) => {
                                    // request completed
                                }
                            }

                            active_connections.fetch_sub(
                                1,
                                Ordering::SeqCst
                            );
                        });
                    }

                    Err(e) => {
                        eprintln!(
                            "Accept error: {}",
                            e
                        );
                    }
                }
            }
        }
    }

    // Drain active requests
    println!("{}", "[GRITSHIELD] Draining active connections...".yellow());

    while active_connections.load(Ordering::SeqCst) > 0 {
        sleep(Duration::from_millis(100)).await;
    }

    println!("{}", "[GRITSHIELD] Shutdown complete".green().bold());
}

async fn handle_connection(mut stream: TcpStream, router: Arc<Router>) {
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
        start_time,
        req,
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

    // Route Execution
    let response = match routing_result {
        RoutingResult::Found(handler, _) => {
            // Process handler with our loaded and mutated context manager
            let response: Response = handler(ctx.clone()).await;

            if router.use_logger {
                router.log_lifecycle(&ctx, response.status, start_time.elapsed());
            }

            router.run_after_hooks(ctx, response.status, start_time.elapsed());

            response
        }
        RoutingResult::NotFound => Response::new(404, Sanitizer::trust("<h1>404</h1>")),
        RoutingResult::MethodNotAllowed => Response::new(405, Sanitizer::trust("<h1>405</h1>")),
    };

    // Send output back over socket wire
    let (bytes, mime) = response.resolve();
    let _ = stream.write_all(&response.to_bytes(&bytes, &mime)).await;
}
