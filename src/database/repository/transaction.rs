use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbErr, ExecResult, QueryResult,
    Statement, TransactionTrait,
};
use sea_orm_migration::async_trait::async_trait;
use std::sync::Arc;
use tokio::task_local;

// SeaORM's DatabaseTransaction is safe to share across tasks
task_local! {
    pub static CURRENT_TXN: Arc<DatabaseTransaction>;
}

/// Helper that begins a transaction, scopes it into `CURRENT_TXN`,
/// and handles Commit on success or Rollback on error/panic.
pub async fn run_in_transaction<F, R, E>(db: &DatabaseConnection, fut: F) -> Result<R, E>
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
                    )
                    .into());
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

pub enum RepositoryConnection {
    Pool(DatabaseConnection),
    Transaction(Arc<DatabaseTransaction>),
}

#[async_trait]
impl ConnectionTrait for RepositoryConnection {
    fn get_database_backend(&self) -> sea_orm::DbBackend {
        match self {
            RepositoryConnection::Pool(db) => db.get_database_backend(),
            RepositoryConnection::Transaction(txn) => txn.get_database_backend(),
        }
    }

    async fn execute(&self, stmt: Statement) -> Result<ExecResult, DbErr> {
        match self {
            RepositoryConnection::Pool(db) => db.execute(stmt).await,
            RepositoryConnection::Transaction(txn) => txn.execute(stmt).await,
        }
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        match self {
            RepositoryConnection::Pool(db) => db.execute_unprepared(sql).await,
            RepositoryConnection::Transaction(txn) => txn.execute_unprepared(sql).await,
        }
    }

    async fn query_one(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        match self {
            RepositoryConnection::Pool(db) => db.query_one(stmt).await,
            RepositoryConnection::Transaction(txn) => txn.query_one(stmt).await,
        }
    }

    async fn query_all(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        match self {
            RepositoryConnection::Pool(db) => db.query_all(stmt).await,
            RepositoryConnection::Transaction(txn) => txn.query_all(stmt).await,
        }
    }
}

impl RepositoryConnection {
    /// Returns Some(&Arc<DatabaseTransaction>) if running inside a transaction scope
    pub fn transaction(&self) -> Option<&Arc<DatabaseTransaction>> {
        match self {
            RepositoryConnection::Transaction(txn) => Some(txn),
            RepositoryConnection::Pool(_) => None,
        }
    }

    /// Returns Some(&DatabaseConnection) if running against the pool directly
    pub fn pool(&self) -> Option<&DatabaseConnection> {
        match self {
            RepositoryConnection::Pool(db) => Some(db),
            RepositoryConnection::Transaction(_) => None,
        }
    }

    /// Check if currently executing within a transaction
    pub fn is_transaction(&self) -> bool {
        matches!(self, RepositoryConnection::Transaction(_))
    }
}

pub trait TxnRepository {
    fn db(&self) -> &sea_orm::DatabaseConnection;

    /// Synchronously resolves the active transaction or defaults to the connection pool
    fn conn(&self) -> RepositoryConnection {
        CURRENT_TXN
            .try_with(|txn| RepositoryConnection::Transaction(txn.clone()))
            .unwrap_or_else(|_| RepositoryConnection::Pool(self.db().clone()))
    }
}

impl RepositoryConnection {
    pub fn as_ref(&self) -> &dyn ConnectionTrait {
        match self {
            RepositoryConnection::Pool(db) => db,
            RepositoryConnection::Transaction(txn) => txn.as_ref(),
        }
    }
}
