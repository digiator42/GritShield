// src/repositories/product.rs
use gritshield::GritAdmin;
use sea_orm::DatabaseConnection;

/// The admin-facing half of the `products` model.
///
/// `GritAdmin` generates the trait impls the panel handlers need
/// (`GritRepository`, `TxnRepository`), the grid column definitions, the
/// per-column read/write rules, and the `#[ctor]` hook that registers
/// `/admin/product/...` in the model registry. Note the single `db` field:
/// every generated handler reconstructs the repository as
/// `ProductRepository { db: (*db).clone() }`, so a second required field has
/// nowhere to come from and fails to compile at the derive.
#[derive(Clone, GritAdmin)]
#[repository(
    searchable = ["sku", "name"],
    grid_columns = ["id", "sku", "name", "price_cents", "created_at"],
    read_only = ["created_at"],
)]
pub struct ProductRepository {
    pub db: DatabaseConnection,
}