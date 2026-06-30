// src/repository/model.rs
use crate::repository::jpa_dsl::{generate_model_specific_methods, type_to_column_type};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Ident, Meta, Path, Result};

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

pub fn expand_model(input: DeriveInput) -> Result<TokenStream> {
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            syn::Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "GritModel only supports named field structures",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "GritModel can only be derived on Structs",
            ))
        }
    };

    let mut explicit_repo_path: Option<Path> = None;
    let mut sea_orm_table_name: Option<String> = None;

    // Parse all necessary attributes in a single pass
    for attr in &input.attrs {
        if attr.path().is_ident("grit") {
            if let Meta::List(meta_list) = &attr.meta {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("repository") {
                        let value = meta.value()?;
                        let path_str: syn::LitStr = value.parse()?;
                        explicit_repo_path = Some(path_str.parse::<Path>()?);
                    }
                    Ok(())
                });
            }
        } else if attr.path().is_ident("sea_orm") {
            if let Meta::List(meta_list) = &attr.meta {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("table_name") {
                        let value = meta.value()?;
                        let table_str: syn::LitStr = value.parse()?;
                        sea_orm_table_name = Some(table_str.value());
                    }
                    Ok(())
                });
            }
        }
    }

    // Resolve the repository path: explicit input OR inferred fallback via convention
    let repo_path: Path = match explicit_repo_path {
        Some(path) => path,
        None => {
            if let Some(table) = sea_orm_table_name {
                // Simple singularization rule (e.g., "users" -> "user", "posts" -> "post")
                let singular_mod_name = if table.ends_with('s') {
                    &table[..table.len() - 1]
                } else {
                    &table
                };

                let module_ident = Ident::new(singular_mod_name, Span::call_site());
                let repo_struct_name = format!("{}Repository", to_pascal_case(singular_mod_name));
                let repo_ident = Ident::new(&repo_struct_name, Span::call_site());

                // Construct fully qualified path macro tokens
                syn::parse2(quote! {
                    crate::repositories::#module_ident::#repo_ident
                }).unwrap()
            } else {
                return Err(syn::Error::new_spanned(
                    &input,
                    "GritModel requires either an explicit link attribute `#[grit(repository = \"...\")]` or a baseline `#[sea_orm(table_name = \"...\")]` declaration to infer paths.",
                ));
            }
        }
    };

    let repo_ident = &repo_path.segments.last().unwrap().ident;
    let all_builder_name = Ident::new(&format!("{}RAQB", repo_ident), Span::call_site());

    let mut parsed_fields = Vec::new();
    for field in fields {
        let ident = field.ident.clone().unwrap();
        let col_type = type_to_column_type(&field.ty);
        parsed_fields.push((ident, col_type));
    }

    let entity_path = quote! { Entity };
    let column_path = quote! { Column };
    let all_builder_path = quote! { #all_builder_name };

    let jpa_methods_block = generate_model_specific_methods(
        &entity_path,
        &column_path,
        &all_builder_path,
        &parsed_fields,
    );

    Ok(quote! {
        impl #repo_path {
            #jpa_methods_block
        }
    })
}