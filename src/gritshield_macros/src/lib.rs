extern crate proc_macro;
use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput, ItemImpl, ItemFn, Type};

mod shield;
mod admin;
mod core_parser;
mod ioc;
mod repository;
mod routing;
mod sanitizer;
mod event;
mod job;

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

#[proc_macro_derive(GritComponent)]
pub fn derive_grit_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    ioc::component::expand_grit_component(input)
}

#[proc_macro_derive(GritWire)]
pub fn derive_grit_wire(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    ioc::wire::expand_grit_wire(input)
        .unwrap_or_else(|err| err.to_compile_error())
        .into()
}

#[proc_macro_derive(WireContainer)]
pub fn derive_wire_container(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    ioc::component::expand_wire_container(input).into()
}

#[proc_macro_derive(GritSanitizer, attributes(clean))]
pub fn derive_grit_sanitizer(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    sanitizer::sanitize::expand_grit_sanitizer(input).into()
}

#[proc_macro_derive(GritEvent)]
pub fn derive_grit_event(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    
    // String representation of the struct name for compile-time dispatching
    let event_name_str = name.to_string();
    let expanded = quote! {
        impl #impl_generics ::gritshield::core::event_bus::GritEvent for #name #ty_generics #where_clause {
            fn event_name() -> &'static str {
                #event_name_str
            }
        }

        // Provides zero-import .publish() directly on the event struct!
        impl #impl_generics #name #ty_generics #where_clause {
            /// Waits in transactional context to publish the event after commit
            pub async fn publish(self) {
                use ::gritshield::core::event_bus::GritEventExt;
                <Self as GritEventExt>::publish(self).await;
            }
        }
    };
    TokenStream::from(expanded)
}

#[proc_macro_derive(GritJob, attributes(job))]
pub fn derive_grit_job(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    job::register::expand_derive_grit_job(input).into()
}

// ==========================================
// ATTRIBUTE MACROS (Event/Job Queue & DI)
// ==========================================

#[proc_macro_attribute]
pub fn event(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemImpl);
    event::register::expand_event(input).into()
}

#[proc_macro_attribute]
pub fn job(attr: TokenStream, item: TokenStream) -> TokenStream {
    job::register::expand_job(attr, item)
}

#[proc_macro_attribute]
pub fn action(attr: TokenStream, item: TokenStream) -> TokenStream {
    admin::action::expand_action(attr, item)
}

#[proc_macro_attribute]
pub fn component(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input_impl = parse_macro_input!(item as ItemImpl);
    ioc::component::expand_component(input_impl)
}

// ==========================================
// ATTRIBUTE MACROS (Controllers & Endpoints)
// ==========================================

#[proc_macro_attribute]
pub fn launch(attr: TokenStream, item: TokenStream) -> TokenStream {
    shield::launch::expand_launch(attr, item)
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

#[proc_macro_attribute]
pub fn catch(attr: TokenStream, item: TokenStream) -> TokenStream {
    shield::catch::expand_catch(attr, item)
}

#[proc_macro_attribute]
pub fn intercept(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse the interceptor struct name passed in the attribute, e.g., AuditLogger
    let interceptor_type = parse_macro_input!(attr as Type);
    
    // Parse the target function
    let input_fn = parse_macro_input!(item as ItemFn);

    let vis = &input_fn.vis;
    let sig = &input_fn.sig;
    let body = &input_fn.block;
    let attrs = &input_fn.attrs;
    let fn_name = &sig.ident;

    if sig.asyncness.is_none() {
        return syn::Error::new_spanned(
            sig.fn_token,
            "#[intercept] can only be applied to async functions",
        )
        .to_compile_error()
        .into();
    }

    let expanded = quote! {
        #(#attrs)*
        #vis #sig {
            use ::gritshield::core::aop::{Interceptor, InvocationContext};

            let interceptor = #interceptor_type;

            let ctx = InvocationContext {
                target_name: std::any::type_name::<Self>(),
                method_name: stringify!(#fn_name),
                db: &self.db,
            };

            // Execute the original body as the 'next' closure
            interceptor.intercept(ctx, Box::new(|| {
                Box::pin(async move {
                    #body
                })
            })).await
        }
    };

    TokenStream::from(expanded)
}

/// Convenience macro alias so developers can write #[transactional]
#[proc_macro_attribute]
pub fn transactional(_attr: TokenStream, item: TokenStream) -> TokenStream {
    // Expands #[transactional] to #[intercept(::gritshield::aop::TransactionalInterceptor)]
    let interceptor_path: TokenStream = quote!(::gritshield::core::aop::TransactionalInterceptor).into();
    intercept(interceptor_path, item)
}