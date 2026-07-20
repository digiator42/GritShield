use ::gritshield::deps::futures::FutureExt;
use gritshield::security::rbac::{Admin, Auditor, DeleteUser, ManageBilling, Manager, ViewLogs};
use gritshield::{
    component,
    core::{ioc::AutoWire, logger::Logger, schema::export_openapi, LogLevel},
    database::db::{DbConfig, DbManager},
    declare_security_caps,
    http::request::HttpMethod,
    inject,
    middleware::AuthMiddleware,
    prelude::*,
    routing::engine::BoxedResponse,
    GritComponent, GritWire, WireContainer,
};

mod controllers;
mod models;
mod repositories;
mod auth {
    mod login;
}
mod services;

// One single source of truth grouped by capability matching your endpoint attributes!
declare_security_caps! {
    ManageBilling => [Admin],
    DeleteUser    => [Admin],
    ViewLogs      => [Manager, Auditor],
}

// #[get("/hello")]
// pub async fn system_info(ctx: RequestContext, redis: Arc<OrderController>) -> Response {
//     Response::ok("")
// }

fn auto_wire() {
    // provide!(PaymentService, PaymentService::new("Api key....".to_string()));
    // 1. Setup local environment configurations, preferred to get it from env
    let redis_url = "redis://127.0.0.1:6379/";

    // 2. Instantiate your asynchronous Redis service
    let redis_service = RedisService::new(redis_url).unwrap();

    // use provider!,
    inject!(RedisService, redis_service);
}

// 1. Define Dependencies
pub struct DatabasePool {}

pub struct PaymentService {}

impl PaymentService {
    pub async fn checkout(&self, ctx: RequestContext) -> Response {
        Response::ok("Compile-time safety verified!".to_string())
    }
}

// 2. Define Controller
#[derive(Clone, GritWire)]
pub struct OrderController {
    pub db: Arc<DatabasePool>,
    pub ps: Arc<PaymentService>,
}

// 3. Define Application Container
#[derive(Clone, WireContainer)] // Automatically proves it has dependencies
pub struct AppContainer {
    pub db: Arc<DatabasePool>,
    pub ps: Arc<PaymentService>,
    pub log: Arc<Logger>,
}

fn auto_wire_compile_time() -> Arc<OrderController> {
    // A. Manually assemble dependencies
    let container = AppContainer {
        db: Arc::new(DatabasePool {}),
        ps: Arc::new(PaymentService {}),
        log: Arc::new(Logger {
            level: LogLevel::Debug,
        }),
    };

    // B. Safely build the controller at compile-time (returns Arc<OrderController>)
    let order_controller = OrderController::compile_time_wire(&container);
    order_controller
}

#[tokio::main]
async fn main() {
    let db_config = DbConfig::default();

    let shared_db = DbManager::connect(db_config).await.unwrap();

    let order_controller = auto_wire_compile_time();

    auto_wire();
    // AutoWire::boot_di_container();

    let router = Router::new()
        .route((
            "/api/orders/checkout",
            HttpMethod::GET,
            move |ctx: RequestContext| {
                let oc = order_controller.clone();
                async move { oc.ps.checkout(ctx).await }.boxed()
            },
        ))
        .add_middleware(AuthMiddleware::new_session(
            vec![
                "/auth/login".to_string(),
                "/api/**".to_string(),
                "/admin/**".to_string(),
            ],
            Some("/api/info/sea-orm"),
        ))
        .mount_db(shared_db.clone())
        .add_role_inheritance("Admin", vec!["Manager", "Operator", "Auditor"])
        .add_role_inheritance("Manager", vec!["Editor", "Viewer"])
        .add_role_inheritance("Editor", vec!["Contributor"]);

    export_openapi("target/schema.json").unwrap();

    seed_social_media_if_empty(shared_db.as_ref()).await;

    // Fire the framework runtime loop
    ignite("127.0.0.1", "8080", router).await;
}

use crate::{models::*, services::redis::RedisService};
use chrono::Utc;
use rand::seq::SliceRandom;
use rand::Rng;
use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, PaginatorTrait, Set};

pub async fn seed_social_media_if_empty(db: &DatabaseConnection) {
    let mut rng = rand::thread_rng();

    // ---- Seed Users ----
    let user_count = user::Entity::find().count(db).await.unwrap_or(0);
    if user_count == 0 {
        println!("🌱 Seeding 20 users...");
        for i in 1..=20 {
            let mock_user = user::ActiveModel {
                username: Set(format!("user_{}", i)),
                email: Set(format!("user_{}@example.com", i)),
                created_at: Set(Utc::now().naive_utc()),
                updated_at: Set(Utc::now().naive_utc()),
                ..Default::default()
            };
            mock_user.insert(db).await.expect("Insert user failed");
        }
        println!("✅ Users seeded!");
    }

    // ---- Seed Posts ----
    let post_count = post::Entity::find().count(db).await.unwrap_or(0);
    if post_count == 0 {
        println!("🌱 Seeding 50 posts...");
        let user_ids: Vec<i64> = user::Entity::find()
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|u| u.id)
            .collect();

        for i in 1..=50 {
            let user_id = user_ids[rng.gen_range(0..user_ids.len())];
            let mock_post = post::ActiveModel {
                user_id: Set(user_id as i32),
                content: Set(format!(
                    "Post #{} by user {}: This is a sample social media post!",
                    i, user_id
                )),
                created_at: Set(Utc::now().naive_utc()),
                ..Default::default()
            };
            mock_post.insert(db).await.expect("Insert post failed");
        }
        println!("✅ Posts seeded!");
    }

    // ---- Seed Comments ----
    let comment_count = comment::Entity::find().count(db).await.unwrap_or(0);
    if comment_count == 0 {
        println!("🌱 Seeding 100 comments...");
        let user_ids: Vec<i64> = user::Entity::find()
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|u| u.id)
            .collect();

        let post_ids: Vec<i32> = post::Entity::find()
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();

        for i in 1..=100 {
            let user_id = user_ids[rng.gen_range(0..user_ids.len())];
            let post_id = post_ids[rng.gen_range(0..post_ids.len())];
            let mock_comment = comment::ActiveModel {
                post_id: Set(post_id),
                user_id: Set(user_id as i32),
                content: Set(format!("Comment #{}: Great post! 👍", i)),
                created_at: Set(Utc::now().naive_utc()),
                ..Default::default()
            };
            mock_comment
                .insert(db)
                .await
                .expect("Insert comment failed");
        }
        println!("✅ Comments seeded!");
    }

    // ---- Seed Followers (Social Graph) ----
    let follower_count = follower::Entity::find().count(db).await.unwrap_or(0);
    if follower_count == 0 {
        println!("🌱 Seeding followers (social graph)...");
        let user_ids: Vec<i64> = user::Entity::find()
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|u| u.id)
            .collect();

        let mut follower_pairs = std::collections::HashSet::new();

        // Create a social network: each user follows ~5-10 others
        for &follower_id in &user_ids {
            // Number of people this user follows (3-8)
            let follow_count = rng.gen_range(3..=8);

            // Get random users to follow (excluding self)
            let mut candidates: Vec<i64> = user_ids
                .iter()
                .filter(|&&id| id != follower_id)
                .cloned()
                .collect();

            // Shuffle and pick follow_count users
            candidates.shuffle(&mut rng);
            let followed_users: Vec<i64> = candidates.into_iter().take(follow_count).collect();

            for &followed_id in &followed_users {
                // Avoid duplicates
                let pair = if follower_id < followed_id {
                    (follower_id, followed_id)
                } else {
                    (followed_id, follower_id)
                };

                if !follower_pairs.contains(&pair) {
                    follower_pairs.insert(pair);

                    let mock_follower: follower::ActiveModel = follower::ActiveModel {
                        follower_id: Set(follower_id),
                        followed_id: Set(followed_id),
                        created_at: Set(Utc::now().naive_utc()),
                        ..Default::default()
                    };
                    mock_follower
                        .insert(db)
                        .await
                        .expect("Insert follower failed");
                }
            }
        }
        println!(
            "✅ Followers seeded! {} follow relationships created.",
            follower_pairs.len()
        );
    }

    println!("✅ Social Media seeding complete!");
}
