use gritshield::{
    prelude::*,
    security::db::{DbConfig, DbManager},
};

mod controllers;
mod models;
mod repository;
mod shell;

#[tokio::main]
async fn main() {
    let db_config = DbConfig::default();

    let shared_db = DbManager::connect(db_config).await.unwrap();

    let router = Router::new().mount_db(shared_db);

    // Fire the framework runtime loop
    run_server("127.0.0.1", "8080", router).await;
}
