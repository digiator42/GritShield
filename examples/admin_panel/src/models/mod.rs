//! Entity modules.
//!
//! The module name is not cosmetic. `#[derive(GritModel)]` and
//! `#[derive(GritAdmin)]` both *derive* the paths they need from the table and
//! repository names, and this layout is what they expect:
//!
//! ```text
//! GritModel  on models/product.rs       -> crate::repositories::product::ProductRepository
//! GritAdmin  on repositories/product.rs -> crate::models::product
//! ```
//!
//! Put the entity somewhere else and you get `could not find 'repositories' in
//! the crate root`, with the span pointing at the derive rather than at the
//! path it wanted. Both derives accept an override -- `#[grit(repository =
//! "...")]` and `#[repository(entity = "...")]` -- but note that the value must
//! be a **string**. `#[repository(entity = crate::models)]` does not compile:
//! the attribute list is parsed as `syn::Meta`, whose `NameValue` form only
//! accepts a literal, so you get `error: expected identifier` aimed at the
//! attribute.

pub mod product;