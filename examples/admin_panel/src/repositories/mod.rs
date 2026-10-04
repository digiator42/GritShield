//! Repository modules.
//!
//! `ProductRepository` is the admin-facing half of the pair. Its name is not
//! cosmetic either: `GritAdmin` strips the `Repository` suffix, lowercases what
//! is left, and uses the result for both the entity path
//! (`crate::models::product`) and the route slug (`product`). The generated
//! `ModelMetadata` therefore says `table_slug = "product"`, which is what the
//! dashboard nav and `/admin/product/...` URLs are built from.

pub mod product;