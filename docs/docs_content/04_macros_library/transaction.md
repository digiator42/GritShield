# Transaction Management & Interceptors

GritShield provides clean, declarative transaction management and AOP method interception via the `#[transactional]` and `#[intercept]` attribute macros, powered by task-local context propagation.

## Key Features

- **Zero Boilerplate:** The `#[transactional]` macro automatically handles `BEGIN`, commits on `Ok(())`, and triggers an automatic `ROLLBACK` on `Err(...)` or panic.
    
- **Method Interceptors (`#[intercept]`):** Easily wrap service methods with cross-cutting concerns like logging, audit trailing, or performance profiling using the `Interceptor` trait.
    
- **Driver Agnostic:** Works dynamically across PostgreSQL, MySQL, and SQLite using SeaORM.
    

## Quickstart Example

### 1. Service Definition with Transactions

`#[transactional]` is a short hand interceptor, this expands to inner interceptor like #[intercept(AuditLogger)] in the next example.

Rust

```rust
use gritshield::prelude::*;
use sea_orm::{ActiveModelTrait, DatabaseConnection, DbErr, Set, EntityTrait};
use crate::models::user;

pub struct UserService {
    pub user_repo: UserRepository,
    pub db: DatabaseConnection,
}

impl UserService {
    #[transactional]
    pub async fn create_user(&self, id: i64, email: String, username: String) -> Result<(), DbErr> {
        let new_user = user::ActiveModel {
            id: Set(id),
            username: Set(username),
            email: Set(email),
            ..Default::default()
        };

        // Repository execution: automatically uses the task-local transaction
        // Since we are inside #[transactional], conn will be resolved to DatabaseTransaction
        let conn = self.user_repo.conn();
        user::Entity::insert(new_user)
            .exec(&conn)
            .await?;

        Ok(())
    }
}
```

### 2. Writing Custom Interceptors

Implement the `Interceptor` trait to execute logic before or after method execution. If an error occurs inside `#[transactional]`, outer interceptors can catch the failure after rollback occurs.

Rust

```rust
use gritshield::core::aop::{BoxFuture, Interceptor, InvocationContext};
use gritshield::deps::async_trait;
use sea_orm::DbErr;

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

        if let Err(ref err) = result {
            eprintln!("Method execution failed and rolled back! Error: {:?}", err);
        } else {
            println!("Method execution succeeded!");
        }

        result
    }
}

impl UserService {
    #[intercept(AuditLogger)]
    #[transactional]
    pub async fn create_user(&self, id: i64, email: String, username: String) -> Result<(), DbErr> {
        let new_user = user::ActiveModel {
            id: Set(id),
            username: Set(username),
            email: Set(email),
            ..Default::default()
        };
		
        let conn = self.user_repo.conn();
        user::Entity::insert(new_user)
            .exec(&conn)
            .await?;

        Ok(())
    }
}

```

## How It Works Under the Hood

Plaintext

```rust
UserService::create_user()
 ├── 1. [AuditLogger] Interceptor captures invocation context
 ├── 2. #[transactional] begins a transaction on `self.db`
 ├── 3. Binds active handle to `CURRENT_TXN` (internally) for the async task scope
 ├── 4. `self.user_repo.conn()` returns `RepositoryConnection::Transaction`
 ├── 5. Executes queries over the active transaction handle
 └── 6. Evaluates result:
        ├── Ok(_)  ➜ COMMIT transaction ➜ Interceptor sees Ok
        └── Err(_) ➜ ROLLBACK transaction ➜ Interceptor catches Err
```

## ⚠️ Important Rules

1. **Always execute queries using `.conn()`:** Calling raw queries directly against `&self.db` bypasses `CURRENT_TXN` and executes in auto-commit mode. Always pass `&self.repo.conn()` or `&self.conn()` to SeaORM executors.
    
2. **Task Boundaries (`tokio::spawn`):** Task-local storage (`CURRENT_TXN`) does not cross `tokio::spawn` task boundaries. Avoid spawning detached background tasks inside a `#[transactional]` block if they rely on the active transaction context.