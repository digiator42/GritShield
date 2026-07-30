# Transaction Management

GritShield provides clean, declarative transaction management via the `#[transactional]` attribute macro and task-local storage inspired by Spring boot.

## Key Features

- **Zero Boilerplate:** The `#[transactional]` macro automatically calls `BEGIN`, handles `COMMIT` on `Ok(())`, and triggers an automatic `ROLLBACK` on `Err(...)` or panic.
    
- **Task-Local Context:** The active transaction is stored in `CURRENT_TXN` for the duration of the function execution.
    
- **Driver Agnostic:** Works dynamically across PostgreSQL, MySQL, and SQLite using SeaORM's unified `DatabaseConnection` and `DatabaseTransaction`.
    

## Quickstart Example

### 1. Service Definition

Annotate your service with `#[derive(GritComponent)]`. The `#[transactional]` macro will automatically utilize `self.db_pool` (or `self.db`) to initialize the transaction context.

Rust

```rust
use gritshield::prelude::*;
use sea_orm::{ActiveModelTrait, DatabaseConnection, DbErr};

#[derive(GritComponent)]
pub struct UserService {
    pub user_repo: UserRepository,
    pub db_pool: DatabaseConnection,
}

impl UserService {
    #[transactional]
    pub async fn delete(&self, user: user::ActiveModel) -> Result<(), DbErr> {
        // Retrieve the task-local transaction created by #[transactional]
        let txn = match CURRENT_TXN.try_with(|t| t.clone()) {
            Ok(txn) => txn,
            Err(e) => return Err(DbErr::Custom(e.to_string())),
        };

        // Execute queries using the scoped transaction handle
        let _ = user.delete(txn.as_ref()).await?;

        Ok(())
    }
}
```

## How It Works Under the Hood

Plaintext

```
UserService::delete()
 ├── 1. #[transactional] begins a transaction on `self.db_pool`
 ├── 2. Binds the active handle to `CURRENT_TXN` for the current async task
 ├── 3. Executes the method body (queries use `CURRENT_TXN`)
 └── 4. Evaluates result:
        ├── Ok(_)  ➜ COMMIT transaction
        └── Err(_) ➜ ROLLBACK transaction automatically
```

## ⚠️ Important Rules

1. **Always use `CURRENT_TXN` inside `#[transactional]` methods:**
    
    Calling queries directly against `&self.db_pool` inside a `#[transactional]` method will bypass the active transaction and execute in auto-commit mode.
    
2. **Task Boundaries (`tokio::spawn`):**
    
    Task-local storage (`CURRENT_TXN`) does not automatically cross `tokio::spawn` thread boundaries. Avoid spawning detached background tasks inside a `#[transactional]` block if they rely on the same database transaction context.