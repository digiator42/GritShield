use cron::Schedule;
use proc_macro::TokenStream;
use quote::quote;
use std::str::FromStr;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, DeriveInput, Ident, ItemImpl, LitInt, LitStr, Token,
};

/// Parses parameters inside `#[job(name = "send_welcome_email", retries = 3)]`
#[derive(Default)]
pub struct JobArgs {
    pub name: Option<String>,
    pub retries: u32,
    pub cron: Option<String>,
}

impl Parse for JobArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut name = None;
        let mut retries = 3; // Default 3 retries if omitted
        let mut cron = None;

        while !input.is_empty() {
            let ident: Ident = input.parse()?;
            let _eq: Token![=] = input.parse()?;

            if ident == "name" {
                let lit: LitStr = input.parse()?;
                name = Some(lit.value());
            } else if ident == "retries" {
                let lit: LitInt = input.parse()?;
                retries = lit.base10_parse()?;
            } else if ident == "cron" {
                let lit: LitStr = input.parse()?;
                let cron_expr = lit.value();

                // Compile-time validation: Catches invalid cron expressions at compile-time!
                if let Err(err) = Schedule::from_str(&cron_expr) {
                    return Err(syn::Error::new(
                        lit.span(),
                        format!("Invalid cron expression '{}': {}, example: '0 0 0 * * *'", cron_expr, err),
                    ));
                }

                cron = Some(cron_expr);
            } else {
                return Err(syn::Error::new(
                    ident.span(),
                    format!("Unknown argument `{}` for #[job]", ident),
                ));
            }

            // Handle optional trailing/separating comma
            if input.peek(Token![,]) {
                let _comma: Token![,] = input.parse()?;
            }
        }

        Ok(JobArgs {
            name,
            retries,
            cron,
        })
    }
}

pub fn expand_job(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as JobArgs);
    let input = parse_macro_input!(item as ItemImpl);
    let self_ty = &input.self_ty;

    // Convert Option<String> to Option<&'static str> token expression
    let cron_tokens = match &args.cron {
        Some(c) => quote!(Some(#c)),
        None => quote!(None),
    };

    let job_name = args.name.unwrap_or_else(|| quote!(#self_ty).to_string());
    let handler_type_str = quote!(#self_ty).to_string();
    let max_retries = args.retries;

    let expanded = quote! {
        // 1. Keep developer's inherent impl block
        #input

        // 2. Auto-generate GritJob trait implementation
        #[::gritshield::deps::sea_orm_migration::async_trait::async_trait]
        impl ::gritshield::core::job_queue::GritJob for #self_ty {
            const NAME: &'static str = #job_name;

            fn max_retries(&self) -> u32 {
                #max_retries
            }

            async fn perform(&self) -> Result<(), String> {
                #self_ty::perform(self).await
            }
        }

        impl #self_ty {
            pub async fn enqueue(&self) -> Result<String, String> {
                use ::gritshield::core::job_queue::GritJobExt;
                <Self as GritJobExt>::enqueue(self).await
            }

            pub async fn enqueue_in(&self, delay: ::std::time::Duration) -> Result<String, String> {
                use ::gritshield::core::job_queue::GritJobExt;
                <Self as GritJobExt>::enqueue_in(self, delay).await
            }
        }

        // 3. Compile-time registration with full metadata
        ::gritshield::inventory::submit! {
            ::gritshield::core::job_queue::JobRegistration {
                job_type: #job_name,
                handler_type: #handler_type_str,
                max_retries: #max_retries,
                cron: #cron_tokens, // Some("0 0 * * *") or None
                execute: |payload: &[u8]| {
                    let bytes = payload.to_vec();
                    Box::pin(async move {
                        let job: #self_ty = ::serde_json::from_slice(&bytes)
                            .map_err(|e| format!("Job payload deserialization error: {}", e))?;
                        job.perform().await
                    })
                },
            }
        }
    };

    TokenStream::from(expanded)
}

pub fn expand_derive_grit_job(input: DeriveInput) -> TokenStream {
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let expanded = quote! {
        // Marker or helper impls for GritJob metadata
        impl #impl_generics #name #ty_generics #where_clause {
            pub fn __grit_job_type_name() -> &'static str {
                stringify!(#name)
            }
        }
    };

    TokenStream::from(expanded)
}
