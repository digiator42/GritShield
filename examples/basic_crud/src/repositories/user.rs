use crate::models::user;
use gritshield::database::TxnRepository;
use gritshield::{database::repository::transaction::CURRENT_TXN, transactional, GritAdmin};
use gritshield::{intercept, GritComponent};
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
    pub async fn create(&self, model: user::ActiveModel) -> Result<user::Model, DbErr> {
        if let Ok(txn) = CURRENT_TXN.try_with(|t| t.clone()) {
            // Executing on active transaction
            model.insert(txn.as_ref()).await
        } else {
            // Executing on pool connection
            model.insert(&self.db).await
        }
    }
}
