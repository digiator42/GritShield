use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::{Data, DeriveInput, Ident, LitStr, Meta, Path, Result};
use quote::ToTokens;

pub fn expand_relation(input: DeriveInput) -> Result<TokenStream> {
    let variants = match &input.data {
        Data::Enum(e) => &e.variants,
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "GritRelation can only be derived on Enums",
            ))
        }
    };

    // ---- Extract table name from the enum's attributes ----
    let mut table_name: Option<String> = None;
    
    for attr in &input.attrs {
        if attr.path().is_ident("grit") {
            if let Meta::List(meta_list) = &attr.meta {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("table") {
                        let value = meta.value()?;
                        let lit_str: LitStr = value.parse()?;
                        table_name = Some(lit_str.value());
                    }
                    Ok(())
                });
            }
        }
        if attr.path().is_ident("sea_orm") {
            if let Meta::List(meta_list) = &attr.meta {
                let _ = meta_list.parse_nested_meta(|meta| {
                    if meta.path.is_ident("table_name") {
                        let value = meta.value()?;
                        let lit_str: LitStr = value.parse()?;
                        table_name = Some(lit_str.value());
                    }
                    Ok(())
                });
            }
        }
    }

    let table_name_str = table_name.ok_or_else(|| {
        syn::Error::new_spanned(
            &input,
            "GritRelation requires a table name. Add #[grit(table = \"your_table\")] or #[sea_orm(table_name = \"your_table\")] to the enum.",
        )
    })?;

    // ---- Create a unique registration function name ----
    let register_fn_name = Ident::new(
        &format!("register_relations_{}", table_name_str),
        proc_macro2::Span::call_site(),
    );

    // Standardized static names
    let all_builder_name = Ident::new("GritAllQueryBuilder", Span::call_site());
    let one_builder_name = Ident::new("GritOneQueryBuilder", Span::call_site());
    let extended_ident = Ident::new("GritRepositoryRecord", Span::call_site());

    let entity_module = quote! { self };

    let mut parsed_has_many: Vec<(String, Path)> = Vec::new();
    let mut parsed_has_one: Vec<(String, Path)> = Vec::new();
    let mut parsed_belongs_to: Vec<(String, Path, Option<String>)> = Vec::new(); // (field, target_path, foreign_key)

    for variant in variants {
        let variant_field_name = variant.ident.to_string().to_lowercase();

        for attr in &variant.attrs {
            if attr.path().is_ident("sea_orm") {
                if let Meta::List(meta_list) = &attr.meta {
                    let _ = meta_list.parse_nested_meta(|meta| {
                        if meta.path.is_ident("has_many") {
                            let value = meta.value()?;
                            let path_str: LitStr = value.parse()?;
                            if let Ok(target_path) = path_str.parse::<Path>() {
                                let field = format!("{}s", variant_field_name);
                                parsed_has_many.push((field, target_path));
                            }
                        } else if meta.path.is_ident("has_one") {
                            let value = meta.value()?;
                            let path_str: LitStr = value.parse()?;
                            if let Ok(target_path) = path_str.parse::<Path>() {
                                parsed_has_one.push((variant_field_name.clone(), target_path));
                            }
                        } else if meta.path.is_ident("belongs_to") {
                            // Parse the full belongs_to with from/to
                            let mut target_path: Option<Path> = None;
                            let mut foreign_key: Option<String> = None;
                            
                            // Parse the nested meta inside belongs_to
                            let _ = meta.parse_nested_meta(|nested_meta| {
                                if nested_meta.path.is_ident("from") {
                                    let value = nested_meta.value()?;
                                    let path: syn::Path = value.parse()?;
                                    if let Some(segment) = path.segments.last() {
                                        foreign_key = Some(segment.ident.to_string());
                                    }
                                } else if nested_meta.path.is_ident("to") {
                                    // The 'to' field is just the target column, ignore for now
                                    let _ = nested_meta.value()?;
                                } else {
                                    // If no path specified, it might be the direct path
                                    // Try to parse it as a path
                                    if let Ok(path) = syn::parse_str::<Path>(&nested_meta.path.clone().into_token_stream().to_string()) {
                                        target_path = Some(path);
                                    }
                                }
                                Ok(())
                            });

                            // If no target_path was found via nested parsing, try parsing the direct value
                            if target_path.is_none() {
                                let value = meta.value()?;
                                if let Ok(path) = value.parse::<Path>() {
                                    target_path = Some(path);
                                }
                            }

                            if let Some(path) = target_path {
                                parsed_belongs_to.push((variant_field_name.clone(), path, foreign_key));
                            }
                        }
                        Ok(())
                    });
                }
            }
        }
    }

    // ---- Build RelationSchema for each parsed relation ----
    let table_name_lit = LitStr::new(&table_name_str, proc_macro2::Span::call_site());
    
    let mut relations = Vec::new();

    // HasMany relations
    for (field, target_path) in &parsed_has_many {
        let target_table = if let Some(last) = target_path.segments.last() {
            last.ident.to_string()
        } else {
            "unknown".to_string()
        };
        let target_table_lit = LitStr::new(&target_table, proc_macro2::Span::call_site());
        let field_lit = LitStr::new(field, proc_macro2::Span::call_site());
        relations.push(quote! {
            ::gritshield::core::schema::RelationSchema {
                kind: ::gritshield::core::schema::RelationKind::HasMany,
                target_table: #target_table_lit.to_string(),
                foreign_key: ::std::option::Option::None,
            }
        });
    }

    // HasOne relations
    for (field, target_path) in &parsed_has_one {
        let target_table = if let Some(last) = target_path.segments.last() {
            last.ident.to_string()
        } else {
            "unknown".to_string()
        };
        let target_table_lit = LitStr::new(&target_table, proc_macro2::Span::call_site());
        relations.push(quote! {
            ::gritshield::core::schema::RelationSchema {
                kind: ::gritshield::core::schema::RelationKind::HasOne,
                target_table: #target_table_lit.to_string(),
                foreign_key: ::std::option::Option::None,
            }
        });
    }

    // BelongsTo relations (with foreign key)
    for (field, target_path, foreign_key) in &parsed_belongs_to {
        let target_table = if let Some(last) = target_path.segments.last() {
            last.ident.to_string()
        } else {
            "unknown".to_string()
        };
        let target_table_lit = LitStr::new(&target_table, proc_macro2::Span::call_site());
        let fk_lit = foreign_key.as_ref().map(|s| LitStr::new(s, proc_macro2::Span::call_site()));
        relations.push(quote! {
            ::gritshield::core::schema::RelationSchema {
                kind: ::gritshield::core::schema::RelationKind::BelongsTo,
                target_table: #target_table_lit.to_string(),
                foreign_key: #fk_lit.map(|s| s.to_string()),
            }
        });
    }

    // ---- Registration function that adds relations to the schema registry ----
    let registration = quote! {
        #[::gritshield::startup::ctor(unsafe)]
        fn #register_fn_name() {
            let relations = vec![ #(#relations),* ];
            println!("===== GritRelation: Registering {} relations for table '{}'", relations.len(), #table_name_lit);
            ::gritshield::core::schema::add_relations(#table_name_lit, relations);
        }
    };

    // ---- Build the query builders ----
    let builders_block = crate::repository::query_builders::generate_builders(
        &entity_module,
        &extended_ident,
        &all_builder_name,
        &one_builder_name,
        &parsed_has_many,
        &parsed_has_one,
    );

    // ---- Return both the builders and the registration ----
    Ok(quote! {
        #builders_block
        #registration
    })
}