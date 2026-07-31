use std::future::Future;
use std::pin::Pin;
use sea_orm::{DatabaseConnection, DbErr};
use sea_orm_migration::async_trait::async_trait;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Context passed to every interceptor
pub struct InvocationContext<'a> {
    pub target_name: &'static str,
    pub method_name: &'static str,
    pub db: &'a DatabaseConnection,
}

/// The core Interceptor Trait
#[async_trait]
pub trait Interceptor: Send + Sync {
    async fn intercept<'a>(
        &'a self,
        ctx: InvocationContext<'a>,
        next: Box<dyn FnOnce() -> BoxFuture<'a, Result<(), DbErr>> + Send + 'a>,
    ) -> Result<(), DbErr>;
}

/// Built-in Transactional Interceptor using your existing run_in_transaction
pub struct TransactionalInterceptor;

#[async_trait]
impl Interceptor for TransactionalInterceptor {
    async fn intercept<'a>(
        &'a self,
        ctx: InvocationContext<'a>,
        next: Box<dyn FnOnce() -> BoxFuture<'a, Result<(), DbErr>> + Send + 'a>,
    ) -> Result<(), DbErr> {
        // Runs the `next` pipeline inside your existing run_in_transaction helper!
        crate::database::run_in_transaction(ctx.db, async move {
            next().await
        }).await
    }
}