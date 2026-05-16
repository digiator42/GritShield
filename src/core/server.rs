use colored::*;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};

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

    match Request::parse(&mut stream).await {
        Ok(req) => match router.run_middlewares(&req) {
            MiddlewareResult::Error(err_res) => {
                let (bytes, mime) = err_res.resolve();

                let _ = stream.write_all(&err_res.to_bytes(&bytes, &mime)).await;

                if router.use_logger {
                    log_request_summary(&req, err_res.status, start_time.elapsed(), None, None);
                }

                return;
            }

            MiddlewareResult::Next(session_state) => {
                let routing_result = router.match_route(&req.method, &req.path);

                let response = match routing_result {
                    RoutingResult::Found(handler, params) => {
                        let (session_ptr, claims_ptr, is_new_session) = match session_state {
                            Some(state) => (state.session, state.claims, false),
                            None => (None, None, false),
                        };

                        let form = req.parse_form_body();

                        let session_id_log =
                            session_ptr.as_ref().map(|s| s.lock().unwrap().id.clone());

                        let jwt_sub_log = claims_ptr.as_ref().map(|c| c.sub.clone());

                        let ctx = RequestContext {
                            params,
                            headers: req.headers.clone(),
                            claims: claims_ptr,
                            query: req.query.clone(),
                            session: session_ptr.clone(),
                            form,
                            db: router.db.clone(),
                            raw_body: req.body.clone(),
                            content_type: req.headers.get("content-type").cloned(),
                        };

                        let mut response: Response = handler(ctx).await;

                        if router.use_logger {
                            log_request_summary(
                                &req,
                                response.status,
                                start_time.elapsed(),
                                session_id_log,
                                jwt_sub_log,
                            );
                        }

                        if is_new_session {
                            if let Some(s) = session_ptr {
                                let sid = s.lock().unwrap().id.clone();

                                response
                                    .cookies
                                    .push(crate::protocol::response::Cookie::new(
                                        "session_id",
                                        &sid,
                                    ));
                            }
                        }

                        response
                    }

                    RoutingResult::NotFound => Response::new(404, Sanitizer::trust("<h1>404</h1>")),

                    RoutingResult::MethodNotAllowed => {
                        Response::new(405, Sanitizer::trust("<h1>405</h1>"))
                    }
                };

                let (bytes, mime) = response.resolve();

                let _ = stream.write_all(&response.to_bytes(&bytes, &mime)).await;
            }
        },

        Err(e) => {
            eprintln!("{} {}", "Security Warning:".red().bold(), e);

            let err_res = Response::new(400, Sanitizer::trust("<h1>Bad Request</h1>"));

            let (bytes, mime) = err_res.resolve();

            let _ = stream.write_all(&err_res.to_bytes(&bytes, &mime)).await;
        }
    }
}
