use crate::models::user;
use crate::repositories::user::UserRepository;
use chrono::Utc;
use gritshield::core::aop::{BoxFuture, Interceptor, InvocationContext};
use gritshield::database::TxnRepository;
use gritshield::deps::async_trait;
use gritshield::intercept;
use gritshield::transactional;
use gritshield::GritComponent;
use sea_orm::ActiveValue::Set;
use sea_orm::EntityTrait;
use sea_orm::{DatabaseConnection, DbErr};

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
        
        // Trying to delete user id to check rollback on failure
        user::Entity::delete_by_id(20).exec(&conn).await?;

        let new_user = user::ActiveModel {
            id: Set(id),
            username: Set(format!("user_{}", id)),
            email: Set(email),
            created_at: Set(Utc::now().naive_utc()),
            updated_at: Set(Utc::now().naive_utc()),
        };

        // Leverage your TxnRepository connection directly!
        // Avoid dropping a temporary by binding the repo connection first.
        user::Entity::insert(new_user).exec(&conn).await?;

        Ok(())
    }
}

use std::sync::atomic::{AtomicBool, Ordering};

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
