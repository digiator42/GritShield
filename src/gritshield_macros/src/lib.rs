extern crate proc_macro;
use proc_macro::TokenStream;
use syn::{parse_macro_input, DeriveInput};

mod admin;
mod core_parser;
mod repository;
mod routing;

#[proc_macro_derive(GritAdmin, attributes(repository))]
pub fn derive_grit_admin(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    admin::expand_admin(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

#[proc_macro_derive(GritModel, attributes(grit))]
pub fn derive_grit_model(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    repository::model::expand_model(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

#[proc_macro_derive(GritRelation, attributes(sea_orm, grit))]
pub fn derive_grit_relation(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    repository::relation::expand_relation(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

#[proc_macro_derive(GritSchema)]
pub fn derive_grit_schema(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    repository::schema::expand_schema(input)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

// ==========================================
// ATTRIBUTE MACROS (Controllers & Endpoints)
// ==========================================
#[proc_macro_attribute]
pub fn action(attr: TokenStream, item: TokenStream) -> TokenStream {
    admin::action::expand_action(attr, item)
}

#[proc_macro_attribute]
pub fn controller(attr: TokenStream, item: TokenStream) -> TokenStream {
    routing::expand_controller(attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

#[proc_macro_attribute]
pub fn get(attr: TokenStream, item: TokenStream) -> TokenStream {
    routing::expand_http_method("GET", attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

#[proc_macro_attribute]
pub fn post(attr: TokenStream, item: TokenStream) -> TokenStream {
    routing::expand_http_method("POST", attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

#[proc_macro_attribute]
pub fn put(attr: TokenStream, item: TokenStream) -> TokenStream {
    routing::expand_http_method("PUT", attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

#[proc_macro_attribute]
pub fn patch(attr: TokenStream, item: TokenStream) -> TokenStream {
    routing::expand_http_method("PATCH", attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

#[proc_macro_attribute]
pub fn delete(attr: TokenStream, item: TokenStream) -> TokenStream {
    routing::expand_http_method("DELETE", attr.into(), item.into())
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}
