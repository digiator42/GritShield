# Database Integration

GritShield integrates with SeaORM for type-safe, async database access.

## Supported Databases

- PostgreSQL
- MySQL
- SQLite
- (Any database SeaORM supports)

## Quick Start

```rust
use sea_orm::{Database, DatabaseConnection};
use std::sync::Arc;

let db = Database::connect("postgres://user:pass@localhost/mydb").await?;
let router = Router::new().mound_db(Arc::new(db));
```

## Topics

- SeaORM Integration - Detailed usage
- Migrations - Schema management

## Entity Example

```rust
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct User {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub email: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
```

## Query Example

```rust
#[get("/users")]
async fn list_users(ctx: RequestContext) -> Result<Response, FrameworkError> {
    let db = ctx.db.as_ref().ok_or_else(|| {
        FrameworkError::DatabaseFailure("No database connection".into())
    })?;
    
    let users = User::find().all(db.as_ref()).await
        .map_err(|e| FrameworkError::DatabaseFailure(e.to_string()))?;
    
    Response::json(200, &users)
}
```