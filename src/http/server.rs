use colored::*;

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::time::{sleep, Duration};

use crate::{
    core::job_queue::{CronScheduler, JobWorkerEngine},
    routing::engine::Router,
};
use crate::{error, http::handle_connection, info};

pub async fn ignite(host: &str, port: &str, router: Router) {
    let listener = TcpListener::bind(format!("{}:{}", host, port))
        .await
        .unwrap();

    info!(
        "{} {}:{}",
        "[GRITSHIELD] Server Online at".green().bold(),
        host,
        port
    );

    let router = Arc::new(router);

    // Start Job Worker Engine (Internal jobs now handle their own scopes!)
    let worker_router = router.clone();
    tokio::spawn(async move {
        let worker_engine = JobWorkerEngine::new(
            worker_router.event_bus.clone(),
            worker_router.job_queue.clone(),
            10,
        );
        worker_engine.start().await;
    });

    // Start Cron Scheduler
    let cron_router = router.clone();
    tokio::spawn(async move {
        let cron_scheduler = CronScheduler::new(cron_router.job_queue.clone());
        cron_scheduler.start().await;
    });

    // graceful shutdown broadcaster
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // active connection tracker
    let active_connections = Arc::new(AtomicUsize::new(0));

    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!(
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
                        error!(
                            "Accept error: {}",
                            e
                        );
                    }
                }
            }
        }
    }

    // Drain active requests
    info!("{}", "[GRITSHIELD] Draining active connections...".yellow());

    while active_connections.load(Ordering::SeqCst) > 0 {
        sleep(Duration::from_millis(100)).await;
    }

    info!("{}", "[GRITSHIELD] Shutdown complete".green().bold());
}
