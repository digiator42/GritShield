use proc_macro::TokenStream;
use quote::quote;
use syn::{Data, DataStruct, DeriveInput, Fields, GenericArgument, PathArguments, Type};

/// Helper to check if a field type is `Option<T>`
fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            return segment.ident == "Option";
        }
    }
    false
}

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
        let field_type = &f.ty;
        let is_opt = is_option_type(field_type);
        let mut sanitizers = Vec::new();

        for attr in &f.attrs {
            if attr.path().is_ident("clean") {
                let _ = attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("trim") {
                        if is_opt {
                            sanitizers.push(quote! {
                                if let Some(ref mut val) = self.#field_name {
                                    *val = val.trim().to_string();
                                }
                            });
                        } else {
                            sanitizers.push(quote! {
                                self.#field_name = self.#field_name.trim().to_string();
                            });
                        }
                    } else if meta.path.is_ident("html_escape") {
                        if is_opt {
                            sanitizers.push(quote! {
                                if let Some(ref mut val) = self.#field_name {
                                    *val = ::gritshield::security::xss::Sanitizer::encode(val).into_string();
                                }
                            });
                        } else {
                            sanitizers.push(quote! {
                                self.#field_name = ::gritshield::security::xss::Sanitizer::encode(&self.#field_name).into_string();
                            });
                        }
                    } else if meta.path.is_ident("lowercase") {
                        if is_opt {
                            sanitizers.push(quote! {
                                if let Some(ref mut val) = self.#field_name {
                                    *val = val.to_lowercase();
                                }
                            });
                        } else {
                            sanitizers.push(quote! {
                                self.#field_name = self.#field_name.to_lowercase();
                            });
                        }
                    } else if meta.path.is_ident("uppercase") {
                        if is_opt {
                            sanitizers.push(quote! {
                                if let Some(ref mut val) = self.#field_name {
                                    *val = val.to_uppercase();
                                }
                            });
                        } else {
                            sanitizers.push(quote! {
                                self.#field_name = self.#field_name.to_uppercase();
                            });
                        }
                    } else if meta.path.is_ident("url_decode") {
                        if is_opt {
                            sanitizers.push(quote! {
                                if let Some(ref mut val) = self.#field_name {
                                    *val = ::gritshield::security::xss::Sanitizer::url_decode(val);
                                }
                            });
                        } else {
                            sanitizers.push(quote! {
                                self.#field_name = ::gritshield::security::xss::Sanitizer::url_decode(&self.#field_name);
                            });
                        }
                    } else if meta.path.is_ident("nested") {
                        if is_opt {
                            sanitizers.push(quote! {
                                if let Some(ref mut val) = self.#field_name {
                                    val.sanitize();
                                }
                            });
                        } else {
                            sanitizers.push(quote! {
                                self.#field_name.sanitize();
                            });
                        }
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