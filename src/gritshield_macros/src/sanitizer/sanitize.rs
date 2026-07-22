use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Fields};

pub fn expand_grit_sanitizer(input: DeriveInput) -> TokenStream {
    let name = input.ident;

    let fields = match input.data {
        Data::Struct(DataStruct {
            fields: Fields::Named(fields),
            ..
        }) => fields.named,
        _ => panic!("GritSanitizer can only be derived on structs with named fields"),
    };

    let field_sanitizations = fields.iter().map(|f| {
        let field_name = &f.ident;
        let mut sanitizers = Vec::new();

        for attr in &f.attrs {
            if attr.path().is_ident("clean") {
                let _ = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("trim") {
                        sanitizers.push(quote! {
                            self.#field_name = self.#field_name.trim().to_string();
                        });
                    } else if meta.path.is_ident("html_escape") {
                        sanitizers.push(quote! {
                            self.#field_name = ::gritshield::security::xss::Sanitizer::encode(&self.#field_name).into_string();
                        });
                    } else if meta.path.is_ident("lowercase") {
                        sanitizers.push(quote! {
                            self.#field_name = self.#field_name.to_lowercase();
                        });
                    } else if meta.path.is_ident("uppercase") {
                        sanitizers.push(quote! {
                            self.#field_name = self.#field_name.to_uppercase();
                        });
                    } else if meta.path.is_ident("url_decode") {
                        sanitizers.push(quote! {
                            self.#field_name = ::gritshield::security::xss::Sanitizer::url_decode(&self.#field_name);
                        });
                    } else if meta.path.is_ident("nested") {
                        sanitizers.push(quote! {
                            self.#field_name.sanitize();
                        });
                    }
                    Ok(())
                });
            }
        }

        quote! {
            #(#sanitizers)*
        }
    });

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let expanded = quote! {
        impl #impl_generics ::gritshield::security::sanitizer::GritSanitizable for #name #ty_generics #where_clause {
            fn sanitize(&mut self) {
                #(#field_sanitizations)*
            }
        }
    };

    TokenStream::from(expanded)
}