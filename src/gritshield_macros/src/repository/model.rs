// src/repository/model.rs
use crate::repository::jpa_dsl::{generate_model_specific_methods, type_to_column_type};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Ident, Meta, Path, Result};

pub fn expand_model(input: DeriveInput) -> Result<TokenStream> {
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            syn::Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "GritModel only supports named fields",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "GritModel must be a Struct",
            ))
        }
    };

    let mut repo_path: Option<Path> = None;
    let mut table_name: Option<String> = None;

    for attr in &input.attrs {
        if attr.path().is_ident("grit") {
            if let Meta::List(meta_list) = &attr.meta {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("repository") {
                        let value = meta.value()?;
                        let path_str: syn::LitStr = value.parse()?;
                        repo_path = Some(path_str.parse::<Path>()?);
                    }
                    Ok(())
                });
            }
        } else if attr.path().is_ident("sea_orm") {
            if let Meta::List(meta_list) = &attr.meta {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("table_name") {
                        let value = meta.value()?;
                        let lit_str: syn::LitStr = value.parse()?;
                        table_name = Some(lit_str.value());
                    }
                    Ok(())
                });
            }
        }
    }

    let repo_path = match repo_path {
        Some(path) => path,
        None => {
            let table_str = table_name.ok_or_else(|| {
                syn::Error::new_spanned(
                    &input,
                    "GritModel requires a table_name to derive repository path",
                )
            })?;
            let module_name = if table_str.ends_with('s') && table_str.len() > 1 {
                &table_str[..table_str.len() - 1]
            } else {
                &table_str
            };
            let mut chars = module_name.chars();
            let pascal_repo = chars.next().unwrap().to_uppercase().collect::<String>()
                + chars.as_str()
                + "Repository";
            let derived_path_str = format!("crate::repositories::{}::{}", module_name, pascal_repo);
            syn::parse_str::<Path>(&derived_path_str)?
        }
    };

    let mut parsed_fields = Vec::new();
    for field in fields {
        let ident = field.ident.clone().unwrap();
        let col_type = type_to_column_type(&field.ty);
        parsed_fields.push((ident, col_type));
    }

    // Reference the standardized local builder name directly!
    let entity_path = quote! { Entity };
    let column_path = quote! { Column };
    let all_builder_path = quote! { GritAllQueryBuilder };

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
