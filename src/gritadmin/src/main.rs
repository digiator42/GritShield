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

    let router = Router::new().mount_db(shared_db.clone());

    seed_test_users_if_empty(shared_db.as_ref()).await;

    // Fire the framework runtime loop
    run_server("127.0.0.1", "8080", router).await;
}

use crate::models::user;
use crate::models::admin;
use sea_orm::{ActiveModelTrait, ConnectionTrait, DatabaseConnection, EntityTrait, PaginatorTrait}; // Adjust path to your user entity module

async fn seed_test_users_if_empty(db: &DatabaseConnection) {
    // 1. Force execute our strict structural migration schema layout
    let schema_sql = r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE,
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE TABLE IF NOT EXISTS admins (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE,
            password_hash TEXT, -- Allow NULL if omitted in seeder
            created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
    "#;

    // Execute DDL statement directly
    let _ = db
        .execute(sea_orm::Statement::from_string(
            db.get_database_backend(),
            schema_sql.to_string(),
        ))
        .await;

    // 2. Now run your existing check and insert loop safely!
    let count = user::Entity::find().count(db).await.unwrap_or(0);
    if count == 0 {
        println!("🌱 Database table verified via migration! Seeding 40 users...");
        for i in 1..=40 {
            let mock_user = user::ActiveModel {
                username: sea_orm::Set(format!("test_user_{}", i)),
                email: sea_orm::Set(format!("user_{}@gritshield.io", i)),
                ..Default::default()
            };

            let mock_admins_user = admin::ActiveModel {
                username: sea_orm::Set(format!("test_admin_{}", i)),
                email: sea_orm::Set(format!("admin_{}@gritshield.io", i)),
                ..Default::default()
            };

            match mock_user.insert(db).await {
                Ok(_) => println!("Inserted user {}", i),
                Err(err) => eprintln!("❌ Seeding failed at user {}: {:?}", i, err),
            }

            match mock_admins_user.insert(db).await {
                Ok(_) => println!("Inserted admin {}", i),
                Err(err) => eprintln!("❌ Seeding failed at admin {}: {:?}", i, err),
            }
        }
        println!("✅ Seeding complete.");
    }
}
