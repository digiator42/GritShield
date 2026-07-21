use proc_macro::TokenStream;
use quote::quote;
use syn::{GenericArgument, ImplItem, ItemImpl, PathArguments, Type, TypePath};

pub fn expand_event(input: ItemImpl) -> TokenStream {
    let self_ty = &input.self_ty;

    // 1. Inspect the impl block to find the `handle` method and extract the event type
    let mut event_type: Option<Type> = None;

    for item in &input.items {
        if let ImplItem::Fn(method) = item {
            if method.sig.ident == "handle" {
                // Inspect the second parameter: handle(&self, event: Arc<UserRegistered>)
                if let Some(syn::FnArg::Typed(pat_type)) = method.sig.inputs.iter().nth(1) {
                    event_type = Some(extract_inner_event_type(&pat_type.ty));
                }
            }
        }
    }

    let event_ty = match event_type {
        Some(ty) => ty,
        None => {
            return syn::Error::new_spanned(
                &input,
                "#[event_handler] requires an async method `handle(&self, event: Arc<Event>)` inside the impl block.",
            )
            .to_compile_error()
            .into();
        }
    };

    // 2. Expand: Keep original impl + auto-gen GritEventHandler + inventory submission
    let expanded = quote! {
        // Retain original impl block
        #input

        // Auto-generate the GritEventHandler trait impl
        #[::gritshield::deps::sea_orm_migration::async_trait::async_trait]
        impl ::gritshield::core::event_bus::GritEventHandler<#event_ty> for #self_ty {
            async fn handle(&self, event: ::std::sync::Arc<#event_ty>) {
                #self_ty::handle(self, event).await;
            }
        }

        // Auto-register into global inventory / DI container at compile-time
        gritshield::inventory::submit! {
            ::gritshield::core::event_bus::EventRegistration {
                register: |bus| {
                    bus.register_handler::<#event_ty, #self_ty>(#self_ty);
                }
            }
        }
    };

    TokenStream::from(expanded)
}

/// Helper function to extract `UserRegistered` from `Arc<UserRegistered>` or raw `UserRegistered`
fn extract_inner_event_type(ty: &Type) -> Type {
    if let Type::Path(TypePath { path, .. }) = ty {
        if let Some(segment) = path.segments.last() {
            if segment.ident == "Arc" {
                if let PathArguments::AngleBracketed(args) = &segment.arguments {
                    if let Some(GenericArgument::Type(inner_ty)) = args.args.first() {
                        return inner_ty.clone();
                    }
                }
            }
        }
    }
    ty.clone()
}