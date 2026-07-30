use crate::models::user;
use gritshield::GritComponent;
use gritshield::{database::repository::transaction::CURRENT_TXN, transactional, GritAdmin};
use sea_orm::ActiveModelTrait;
use sea_orm::EntityTrait;
use sea_orm::{ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbErr, DeleteResult};
use std::sync::Arc;

#[derive(Clone, GritAdmin, GritComponent)]
#[repository(
    searchable = ["username", "email", "created_at", "updated_at"],
    grid_columns = ["id", "email", "username", "created_at", "updated_at"],
    read_only = ["created_at"],
)]
pub struct UserRepository {
    pub db: DatabaseConnection,
}

impl UserRepository {
    /// Returns the active task-local transaction if it exists;
    /// otherwise, falls back to the main connection pool.
    pub async fn conn(&self) -> Option<Arc<DatabaseTransaction>> {
        if let Ok(txn) = CURRENT_TXN.try_with(|t| t.clone()) {
            // Task-local transaction active -> execute inside transaction
            return Some(txn);
        }
        None
    }

    pub async fn create(&self, model: user::ActiveModel) -> Result<user::Model, DbErr> {
        if let Ok(txn) = CURRENT_TXN.try_with(|t| t.clone()) {
            // Executing on active transaction
            model.insert(txn.as_ref()).await
        } else {
            // Executing on pool connection
            model.insert(&self.db).await
        }
    }
    pub async fn delete(&self, user: user::ActiveModel) -> Result<DeleteResult, DbErr> {
        let txn = self.conn().await.unwrap();
        user.delete(txn.as_ref()).await
    }

    pub async fn _find_by_id(&self, id: i32) -> Result<Option<user::Model>, DbErr> {
        let conn = self.conn().await.unwrap();
        user::Entity::find_by_id(id).one(conn.as_ref()).await
    }
}

#[derive(GritComponent)]
pub struct UserService {
    pub user_repo: UserRepository,
    pub db_pool: DatabaseConnection,
}

impl UserService {
    #[transactional]
    pub async fn delete(&self, user: user::ActiveModel) -> Result<(), DbErr> {
        let txn = match CURRENT_TXN.try_with(|t| t.clone()) {
            Ok(txn) => txn,
            Err(e) => return Err(DbErr::Custom(e.to_string())),
        };

        let _ = user.delete(txn.as_ref()).await?;

        Ok(())
    }
}
