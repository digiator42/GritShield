use proc_macro2::TokenStream;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Result};

pub fn expand_schema(input: DeriveInput) -> Result<TokenStream> {
    let name = &input.ident;
    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => {
                return Err(syn::Error::new_spanned(
                    &input,
                    "GritSchema only supports named fields",
                ))
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &input,
                "GritSchema must be a Struct",
            ))
        }
    };

    let mut field_schemas = Vec::new();
    for field in fields {
        let field_name = field.ident.as_ref().unwrap().to_string();
        let type_str = quote::quote!(#field.ty).to_string();
        let is_option = type_str.contains("Option");
        let type_name = if is_option {
            let inner = type_str.replace("Option<", "").replace(">", "");
            inner
        } else {
            type_str
        };

        let openapi_type = match type_name.as_str() {
            s if s.contains("String") => "String",
            s if s.contains("i64")
                || s.contains("i32")
                || s.contains("u64")
                || s.contains("u32") =>
            {
                "i64"
            }
            s if s.contains("bool") => "bool",
            s if s.contains("DateTime") || s.contains("NaiveDateTime") => "NaiveDateTime",
            _ => "String",
        };

        field_schemas.push(quote! {
            ::gritshield::core::schema::FieldSchema {
                name: #field_name.to_string(),
                type_: #openapi_type.to_string(),
                nullable: #is_option,
                primary_key: false,
            }
        });
    }

    let register_fn_name = syn::Ident::new(
        &format!("register_schema_{}", name),
        proc_macro2::Span::call_site(),
    );
    let table_name_lit = syn::LitStr::new(&name.to_string(), proc_macro2::Span::call_site());

    Ok(quote! {
        #[::gritshield::startup::ctor(unsafe)]
        fn #register_fn_name() {
            // Create fields vector once
            let fields = vec![ #(#field_schemas),* ];
            // Pass fields by reference, or clone it
            ::gritshield::core::schema::register_model_schema(#table_name_lit, fields.clone(), Vec::new());
        }
    })
}
