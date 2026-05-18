use colored::*;

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::time::{Duration, sleep};

use crate::core::connection::handle_connection;
use crate::routing::trie::Router;
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
                    // Bind the second element to `peer_addr` instead of throwing it away with `_`
                    Ok((stream, peer_addr)) => {

                        active_connections
                            .fetch_add(1, Ordering::SeqCst);

                        let router =
                            Arc::clone(&router);

                        let active_connections =
                            Arc::clone(&active_connections);

                        let mut shutdown_rx =
                            shutdown_tx.subscribe();

                        // `peer_addr` is moved cleanly into the async task block here
                        tokio::spawn(async move {

                            tokio::select! {
                                _ = shutdown_rx.recv() => {
                                    // server shutting down
                                }

                                // Pass `peer_addr` directly into your handler function
                                _ = handle_connection(
                                    stream,
                                    peer_addr,
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
