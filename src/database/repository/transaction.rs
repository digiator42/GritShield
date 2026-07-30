use std::sync::Arc;
use sea_orm::{DatabaseConnection, DatabaseTransaction, TransactionTrait, DbErr};
use tokio::task_local;

// SeaORM's DatabaseTransaction is safe to share across tasks
task_local! {
    pub static CURRENT_TXN: Arc<DatabaseTransaction>;
}

/// Helper that begins a transaction, scopes it into `CURRENT_TXN`, 
/// and handles Commit on success or Rollback on error/panic.
pub async fn run_in_transaction<F, R, E>(
    db: &DatabaseConnection,
    fut: F,
) -> Result<R, E>
where
    F: std::future::Future<Output = Result<R, E>>,
    E: From<DbErr>,
{
    // START TRANSACTION (Works on Postgres, SQLite, or MySQL dynamically!)
    let txn = db.begin().await?;
    let shared_txn = Arc::new(txn);

    // SCOPE TASK-LOCAL & EXECUTE
    let result = CURRENT_TXN.scope(shared_txn.clone(), fut).await;

    // COMMIT OR ROLLBACK
    match result {
        Ok(val) => {
            // Unwrap the Arc to commit
            match Arc::try_unwrap(shared_txn) {
                Ok(txn) => {
                    txn.commit().await?;
                }
                Err(_) => {
                    return Err(DbErr::Custom(
                        "Transaction handle held outside scope during commit".into(),
                    ).into());
                }
            }
            Ok(val)
        }
        Err(err) => {
            // If `fut` failed, dropping `shared_txn` or calling rollback handles cleanup
            if let Ok(txn) = Arc::try_unwrap(shared_txn) {
                let _ = txn.rollback().await;
            }
            Err(err)
        }
    }
}