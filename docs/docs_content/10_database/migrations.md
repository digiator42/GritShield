
GritShield uses SeaORM's migration system for schema management.

## Setup

Add to `Cargo.toml`:

```toml
[dependencies]
sea-orm = { version = "0.12", features = ["sqlx-postgres", "runtime-tokio-rustls", "macros"] }
sea-orm-migration = { version = "0.12" }
```

## Create Migration

```bash
cargo run -- generate migration create_users_table
```

## Migration File

`migration/src/m20220101_000001_create_users_table.rs`:

```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Users::Id).integer().not_null().auto_increment().primary_key())
                    .col(ColumnDef::new(Users::Name).string().not_null())
                    .col(ColumnDef::new(Users::Email).string().not_null().unique_key())
                    .col(ColumnDef::new(Users::CreatedAt).timestamp().not_null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Users::Table).to_owned())
            .await
    }
}

#[derive(Iden)]
enum Users {
    Table,
    Id,
    Name,
    Email,
    CreatedAt,
}
```

## Run Migrations

In your application startup:

```rust
use sea_orm_migration::{Migrator, MigratorTrait};

async fn run_migrations(db: &DatabaseConnection) -> Result<(), DbErr> {
    Migrator::up(db, None).await
}
```

## Migration CLI

```bash
# Create new migration
cargo run -- generate migration add_age_column

# Run pending migrations
cargo run -- up

# Rollback last migration
cargo run -- down

# Reset all migrations
cargo run -- fresh
```