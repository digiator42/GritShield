use crate::core::event_bus::{JobEnvelope, JobStorage};
use crate::{core::event_bus::EventBus, GritEvent};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DatabaseTransaction, DbErr, ExecResult, QueryResult,
    Statement, TransactionTrait,
};
use sea_orm_migration::async_trait::async_trait;
use std::{
    any::Any,
    sync::{Arc, Mutex},
};
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
    let txn = db.begin().await?;
    let shared_txn = Arc::new(txn);
    let event_buffer = Arc::new(Mutex::new(Vec::new()));
    let job_buffer = Arc::new(Mutex::new(Vec::new())); // 1. Job staging buffer

    // Bind CURRENT_TXN, TX_EVENT_BUFFER, and TX_JOB_BUFFER
    let result = CURRENT_TXN
        .scope(shared_txn.clone(), {
            TX_EVENT_BUFFER.scope(event_buffer.clone(), {
                TX_JOB_BUFFER.scope(job_buffer.clone(), fut)
            })
        })
        .await;

    match result {
        Ok(val) => {
            // Commit DB transaction
            if let Ok(txn) = Arc::try_unwrap(shared_txn) {
                txn.commit().await?;
            }

            // 2. Flush staged events
            let events = {
                let mut guard = event_buffer.lock().unwrap();
                std::mem::take(&mut *guard)
            };

            let _ = CURRENT_EVENT_BUS.try_with(|bus| {
                for event in events {
                    bus.publish_erased_box(event);
                }
            });

            // 3. Flush staged background jobs (POST-COMMIT)
            let jobs = {
                let mut guard = job_buffer.lock().unwrap();
                std::mem::take(&mut *guard)
            };

            if !jobs.is_empty() {
                let _ = CURRENT_JOB_QUEUE.try_with(|queue| {
                    let queue = queue.clone();
                    tokio::spawn(async move {
                        for job in jobs {
                            let _ = queue.enqueue(job).await;
                        }
                    });
                });
            }

            Ok(val)
        }
        Err(err) => {
            // ROLLBACK: Both event_buffer and job_buffer are dropped!
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

task_local! {
    /// Active EventBus instance for the current task/request scope
    pub static CURRENT_EVENT_BUS: Arc<EventBus>;
}

// Stage events emitted during an active transaction
task_local! {
    pub static TX_EVENT_BUFFER: Arc<Mutex<Vec<Box<dyn Any + Send + Sync>>>>;
}

#[inline(always)]
pub async fn publish_event<E: GritEvent + Clone>(event: E) {
    // Clone the event for staging, leaving the original available
    let event_to_stage = event.clone();

    // Check if inside active transaction scope
    let is_staged = TX_EVENT_BUFFER.try_with(|buffer| {
        let buffer = buffer.clone();
        async move {
            let mut guard = buffer.lock().unwrap();
            guard.push(Box::new(event_to_stage));
        }
    });

    if let Ok(fut) = is_staged {
        fut.await;
        return;
    }

    // Fallback: Publish directly to current request's EventBus
    let _ = CURRENT_EVENT_BUS.try_with(|bus| {
        bus.publish(event);
    });
}

task_local! {
    /// Active JobStorage instance for the current task/connection scope
    pub static CURRENT_JOB_QUEUE: Arc<dyn JobStorage>;
}

task_local! {
    /// Stage background jobs enqueued during an active transaction
    pub static TX_JOB_BUFFER: Arc<Mutex<Vec<JobEnvelope>>>;
}
