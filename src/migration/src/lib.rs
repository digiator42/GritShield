pub use sea_orm_migration::prelude::*;

use crate::migration::src::{_001_create_users_table, _002_create_posts_table};

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(_001_create_users_table::Migration),
            Box::new(_002_create_posts_table::Migration),
        ]
    }
}
