// src/repository/relation.rs
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Ident, Meta, Path, Result};

pub fn expand_relation(input: DeriveInput) -> Result<TokenStream> {
    let variants = match input.data {
        Data::Enum(e) => e.variants,
        _ => {
            return Err(syn::Error::new_spanned(
                input,
                "GritRelation can only be derived on Enums",
            ))
        }
    };

    let mut model_base_name = "User".to_string();
    for attr in &input.attrs {
        if attr.path().is_ident("grit") {
            if let Meta::List(meta_list) = &attr.meta {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("model") {
                        let value = meta.value()?;
                        let lit_str: syn::LitStr = value.parse()?;
                        model_base_name = lit_str.value();
                    }
                    Ok(())
                });
            }
        }
    }

    let all_builder_name = Ident::new(
        &format!("{}RepositoryRAQB", model_base_name),
        Span::call_site(),
    );
    let one_builder_name = Ident::new(
        &format!("{}RepositoryROQB", model_base_name),
        Span::call_site(),
    );
    let extended_ident = Ident::new(
        &format!("{}RepositoryRecord", model_base_name),
        Span::call_site(),
    );

    let entity_module = quote! { self };

    let mut parsed_has_many: Vec<(String, Path)> = Vec::new();
    let mut parsed_has_one: Vec<(String, Path)> = Vec::new();

    for variant in variants {
        let variant_field_name = variant.ident.to_string().to_lowercase();

        for attr in &variant.attrs {
            if attr.path().is_ident("sea_orm") {
                if let Meta::List(meta_list) = &attr.meta {
                    let _ = meta_list.parse_nested_meta(|meta| {
                        if meta.path.is_ident("has_many") {
                            let value = meta.value()?;
                            let path_str: syn::LitStr = value.parse()?;
                            if let Ok(target_path) = path_str.parse::<Path>() {
                                let field = format!("{}s", variant_field_name);
                                parsed_has_many.push((field, target_path));
                            }
                        } else if meta.path.is_ident("has_one") {
                            let value = meta.value()?;
                            let path_str: syn::LitStr = value.parse()?;
                            if let Ok(target_path) = path_str.parse::<Path>() {
                                parsed_has_one.push((variant_field_name.clone(), target_path));
                            }
                        } else if meta.path.is_ident("belongs_to") {
                            // Parse belongs_to and pluralize it to match your builder expectations (.with_users())
                            let value = meta.value()?;
                            let path_str: syn::LitStr = value.parse()?;
                            if let Ok(target_path) = path_str.parse::<Path>() {
                                let field = format!("{}s", variant_field_name);
                                parsed_has_one.push((field, target_path));
                            }
                        }
                        Ok(())
                    });
                }
            }
        }
    }

    let builders_block = crate::repository::query_builders::generate_builders(
        &entity_module,
        &extended_ident,
        &all_builder_name,
        &one_builder_name,
        &parsed_has_many,
        &parsed_has_one,
    );

    Ok(builders_block)
}
