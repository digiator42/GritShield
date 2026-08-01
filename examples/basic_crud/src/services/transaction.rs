use crate::models::user;
use crate::repositories::user::UserRepository;
use chrono::Utc;
use gritshield::core::aop::{BoxFuture, Interceptor, InvocationContext};
use gritshield::database::TxnRepository;
use gritshield::deps::async_trait;
use gritshield::{event, intercept, GritEvent};
use gritshield::{job, transactional, GritJob};
use gritshield::{GritComponent, GritJobExt};
use sea_orm::ActiveValue::Set;
use sea_orm::EntityTrait;
use sea_orm::{DatabaseConnection, DbErr};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, GritEvent)]
pub struct UserRegisteredEvent {
    pub user_id: i64,
    pub email: String,
}

pub struct UserRegisteredHandler;

#[event]
impl UserRegisteredHandler {
    pub async fn handle(&self, event: Arc<UserRegisteredEvent>) {
        println!(
            "🔥 [EVENT FIRED!] User registered event dispatched for ID {}",
            event.user_id
        );
    }
}

// =================================

#[derive(Clone, Serialize, Deserialize, Debug, GritJob)]
pub struct UserRegisteredJob {
    pub email: String,
}

#[job]
impl UserRegisteredJob {
    pub async fn perform(&self) -> Result<(), String> {
        println!(
            "🔥 [JOB FIRED!] User registered job dispatched for email {}",
            self.email
        );
        Ok(())
    }
}

// =================================

#[derive(GritComponent)]
pub struct UserService {
    pub user_repo: UserRepository,
    pub db: DatabaseConnection,
}

impl UserService {
    #[intercept(AuditLogger)]
    #[transactional]
    pub async fn create_user(&self, id: i64, email: String) -> Result<(), DbErr> {
        let conn = self.user_repo.conn();

        // Action 1: Delete user ID 19 (will roll back on error)
        user::Entity::delete_by_id(19).exec(&conn).await?;

        // Stage event in TX_EVENT_BUFFER
        UserRegisteredEvent {
            user_id: id,
            email: email.clone(),
        }
        .publish()
        .await;

        UserRegisteredJob {
            email: email.clone(),
        }
        .enqueue_in(std::time::Duration::from_secs(30))
        .await
        .unwrap();

        let new_user = user::ActiveModel {
            id: Set(id),
            username: Set(format!("user_{}", id)),
            email: Set(email),
            created_at: Set(Utc::now().naive_utc()),
            updated_at: Set(Utc::now().naive_utc()),
        };

        // Action 2: Duplicate key insert (FAILS HERE 💥)
        user::Entity::insert(new_user).exec(&conn).await?;

        Ok(())
    }
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

// Shared state to verify AuditLogger caught the error
pub static AUDIT_LOGGED_FAILURE: AtomicBool = AtomicBool::new(false);

pub struct AuditLogger;

#[async_trait]
impl Interceptor for AuditLogger {
    async fn intercept<'a>(
        &'a self,
        ctx: InvocationContext<'a>,
        next: Box<dyn FnOnce() -> BoxFuture<'a, Result<(), DbErr>> + Send + 'a>,
    ) -> Result<(), DbErr> {
        println!("Before method execution...");

        let result = next().await;

        if result.is_err() {
            eprintln!("Method execution failed and rolled back!");
            // The transaction failed and rolled back! Mark that AuditLogger caught it.
            AUDIT_LOGGED_FAILURE.store(true, Ordering::SeqCst);
            return result;
        }

        println!("Method execution succeeded!");

        result
    }
}
