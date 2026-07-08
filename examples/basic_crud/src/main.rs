use gritshield::{
    core::schema::export_openapi, prelude::*, security::db::{DbConfig, DbManager},
};

mod controllers;
mod models;
mod repositories;

#[tokio::main]
async fn main() {
    let db_config = DbConfig::default();

    let shared_db = DbManager::connect(db_config).await.unwrap();

    let router = Router::new().mount_db(shared_db.clone());

    export_openapi("target/schema.json").unwrap();

    seed_social_media_if_empty(shared_db.as_ref()).await;

    // Fire the framework runtime loop
    run_server("127.0.0.1", "8080", router).await;
}

use crate::models::*;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, PaginatorTrait, Set};

pub async fn seed_social_media_if_empty(db: &DatabaseConnection) {
    // Seed users
    let user_count = user::Entity::find().count(db).await.unwrap_or(0);
    if user_count == 0 {
        println!("🌱 Seeding 10 users...");
        for i in 1..=10 {
            let mock_user = user::ActiveModel {
                username: Set(format!("test_user_{}", i)),
                email: Set(format!("user_{}@example.com", i)),
                ..Default::default()
            };
            mock_user.insert(db).await.expect("Insert user failed");
        }
    }

    // Seed posts
    let post_count = post::Entity::find().count(db).await.unwrap_or(0);
    if post_count == 0 {
        println!("🌱 Seeding posts...");
        for i in 1..=30 {
            let mock_post = post::ActiveModel {
                user_id: Set(((i - 1) % 10 + 1) as i32), // distribute posts among users
                content: Set(format!("This is post {}", i)),
                created_at: Set(Utc::now().naive_utc()),
                ..Default::default()
            };
            mock_post.insert(db).await.expect("Insert post failed");
        }
    }

    // Seed comments
    let comment_count = comment::Entity::find().count(db).await.unwrap_or(0);
    if comment_count == 0 {
        println!("🌱 Seeding comments...");
        for i in 1..=50 {
            let mock_comment = comment::ActiveModel {
                post_id: Set(((i - 1) % 30 + 1) as i32), // distribute comments among posts
                user_id: Set(((i - 1) % 10 + 1) as i32), // random user commenting
                content: Set(format!("Comment {} content", i)),
                created_at: Set(Utc::now().naive_utc()),
                ..Default::default()
            };
            mock_comment
                .insert(db)
                .await
                .expect("Insert comment failed");
        }
    }

    println!("✅ Seeding complete.");
}
