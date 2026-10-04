// src/models/product.rs
use chrono::NaiveDateTime;
use gritshield::{GritModel, GritRelation};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// The `products` table.
///
/// An ordinary SeaORM entity with two derives bolted on, and that is the whole
/// story: `GritModel` publishes the column metadata that makes the grid able to
/// draw columns, and `GritRelation` contributes the query builders the generated
/// repository methods are built on.
///
/// Both derives also emit a `#[ctor]` startup hook that registers this model in
/// a global registry. There is no `register()` call for you to remember, and no
/// ordering requirement -- `Router::new()` reads the registry after startup hooks
/// have run.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, GritModel)]
#[sea_orm(table_name = "products")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub sku: String,
    pub name: String,
    pub price_cents: i64,
    /// Deliberately absent from `grid_columns` in the repository: stored, filled
    /// in, and never rendered. The panel reads columns through SeaORM, so an
    /// omitted column is not a hidden column -- it simply is not editable.
    pub internal_notes: Option<String>,
    pub created_at: NaiveDateTime,
}

/// No associations in this example, so this enum has no variants.
///
/// It still has to exist, and it still has to name the table. `GritRelation` is
/// what generates `GritAllQueryBuilder` / `GritOneQueryBuilder`, and `GritModel`
/// generates repository methods that return those types -- drop the derive and
/// you get `cannot find GritAllQueryBuilder in crate::models::product` pointing
/// at a query method that you did not write.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation, GritRelation)]
#[grit(table = "products")]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}