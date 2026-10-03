pub mod tool;

use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Error, FnArg, ItemFn, LitStr, Token,
};

/// Arguments accepted by `#[mcp_resource(...)]`.
#[derive(Default)]
pub struct ResourceArgs {
    pub uri: Option<LitStr>,
    pub name: Option<LitStr>,
    pub description: Option<LitStr>,
    pub mime_type: Option<LitStr>,
    pub required_role: Option<LitStr>,
}

impl Parse for ResourceArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = ResourceArgs::default();

        while !input.is_empty() {
            let ident = input.parse::<syn::Ident>()?;
            input.parse::<Token![=]>()?;

            match ident.to_string().as_str() {
                "uri" => args.uri = Some(input.parse()?),
                "name" => args.name = Some(input.parse()?),
                "description" => args.description = Some(input.parse()?),
                "mime_type" => args.mime_type = Some(input.parse()?),
                "required_role" => args.required_role = Some(input.parse()?),
                other => {
                    return Err(Error::new(
                        ident.span(),
                        format!(
                            "Unknown argument `{}` for #[mcp_resource]. Expected one of: \
                             uri, name, description, mime_type, required_role",
                            other
                        ),
                    ))
                }
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(args)
    }
}

/// Expand `#[mcp_resource(uri = "...", ...)] async fn read(ctx) -> Result<String, _>`.
///
/// The handler returns the resource body as a `String`; the macro wraps it in
/// [`McpResourceContents`](gritshield::mcp::McpResourceContents) so the transport
/// layer never has to know how a given resource renders itself.
pub fn expand_mcp_resource(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ResourceArgs);
    let input = parse_macro_input!(item as ItemFn);

    match build_resource(args, input) {
        Ok(stream) => stream.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn build_resource(args: ResourceArgs, input: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let fn_ident = &input.sig.ident;

    let uri = require(
        &args.uri,
        fn_ident.span(),
        "#[mcp_resource] requires a `uri`, e.g. uri = \"gritshield://system/metrics\"",
    )?;
    let name = args
        .name
        .unwrap_or_else(|| LitStr::new(&fn_ident.to_string(), fn_ident.span()));
    let description = args
        .description
        .unwrap_or_else(|| LitStr::new("", fn_ident.span()));
    let mime_type = args
        .mime_type
        .unwrap_or_else(|| LitStr::new("text/plain", fn_ident.span()));

    let role_tokens = match &args.required_role {
        Some(role) => quote!(Some(#role)),
        None => quote!(None),
    };

let ctx_binding = context_binding(&input)?;
let arity = count_inputs(&input);

if arity > 1 {
        return Err(Error::new_spanned(
            &input.sig,
            "#[mcp_resource] handlers take at most one parameter: the RequestContext",
        ));
    }

    let takes_ctx = arity == 1;

    // Both arities are supported, but only the matching call may be emitted:
    // a runtime `if` would type-check the unused branch against the wrong
    // signature and fail to compile.
    let call = if !takes_ctx {
        quote!(#fn_ident().await)
    } else if takes_reference(&input) {
        quote!(#fn_ident(&#ctx_binding).await)
    } else {
        quote!(#fn_ident(#ctx_binding).await)
    };

    let expanded = quote! {
        #input

        ::gritshield::inventory::submit! {
            ::gritshield::mcp::resource::McpResourceRegistration {
                uri: #uri,
                name: #name,
                description: #description,
                mime_type: #mime_type,
                required_role: #role_tokens,
                reader: |__gritshield_ctx| ::std::boxed::Box::pin(async move {
                    let #ctx_binding = __gritshield_ctx;
                    let __body = #call
                        .map_err(::gritshield::mcp::protocol::McpError::from)?;

                    ::std::result::Result::Ok(
                        ::gritshield::mcp::resource::McpResourceContents {
                            uri: ::std::string::String::from(#uri),
                            mime_type: ::std::string::String::from(#mime_type),
                            text: ::std::string::String::from(__body),
                        }
                    )
                }),
            }
        }
    };

    Ok(expanded)
}

/// Whether the handler's context parameter is a reference.
fn takes_reference(function: &ItemFn) -> bool {
    match function
        .sig
        .inputs
        .iter()
        .find(|arg| matches!(arg, FnArg::Typed(_)))
    {
        Some(FnArg::Typed(typed)) => matches!(&*typed.ty, syn::Type::Reference(_)),
        _ => false,
    }
}

/// Arguments accepted by `#[mcp_prompt(...)]`.
#[derive(Default)]
pub struct PromptArgs {
    pub name: Option<LitStr>,
    pub description: Option<LitStr>,
    /// Declared arguments, as (name, description, required).
    pub arguments: Vec<(String, String, bool)>,
    pub required_role: Option<LitStr>,
}

impl Parse for PromptArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = PromptArgs::default();

        while !input.is_empty() {
            let ident = input.parse::<syn::Ident>()?;
            input.parse::<Token![=]>()?;

            match ident.to_string().as_str() {
                "name" => args.name = Some(input.parse()?),
                "description" => args.description = Some(input.parse()?),
                "required_role" => args.required_role = Some(input.parse()?),

                // Repeated data cannot use nested syntax here: rustc parses
                // attribute arguments as meta items, where a list entry is
                // either `key = literal` or `key(..)` and a *bare* `key(..)`
                // that is not the sole entry is rejected outright. So the
                // declarations travel as two delimited strings.
                //
                //   arguments   = "summary!,severity?"
                //   descriptions = "summary=One-line incident;severity=Assumed"
                //
                // `!` marks an argument required; `?` or bare means optional.
                "arguments" => {
                    let literal: LitStr = input.parse()?;
                    args.arguments = parse_argument_list(&literal)?;
                }
                "descriptions" => {
                    let literal: LitStr = input.parse()?;
                    apply_descriptions(&mut args.arguments, &literal)?;
                }

                other => {
                    return Err(Error::new(
                        ident.span(),
                        format!(
                            "Unknown argument `{}` for #[mcp_prompt]. Expected one of: \
                             name, description, arguments, descriptions, required_role",
                            other
                        ),
                    ))
                }
            }

            if input.peek(Token![,]) {
                input.parse::<Token![,]>()?;
            }
        }

        Ok(args)
    }
}

/// Parse `"summary!,severity?"` into argument declarations.
fn parse_argument_list(literal: &LitStr) -> syn::Result<Vec<(String, String, bool)>> {
    let mut declared = Vec::new();

    for entry in literal.value().split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }

        let (name, required) = match entry.strip_suffix('!') {
            Some(name) => (name.trim(), true),
            None => match entry.strip_suffix('?') {
                Some(name) => (name.trim(), false),
                None => (entry, false),
            },
        };

        if name.is_empty() {
            return Err(Error::new(
                literal.span(),
                format!("`{}` has no argument name", entry),
            ));
        }

        declared.push((name.to_string(), String::new(), required));
    }

    Ok(declared)
}

/// Parse `"summary=One-line incident;severity=Assumed"` onto declared arguments.
///
/// An unknown name is an error rather than a silent no-op: a typo here would
/// otherwise reach a model as a prompt with a missing description.
fn apply_descriptions(
    declared: &mut Vec<(String, String, bool)>,
    literal: &LitStr,
) -> syn::Result<()> {
    let text = literal.value();
    if text.trim().is_empty() {
        return Ok(());
    }

    if declared.is_empty() {
        return Err(Error::new(
            literal.span(),
            "`descriptions` was given without `arguments`",
        ));
    }

    for pair in text.split(';') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }

        let (name, description) = pair.split_once('=').ok_or_else(|| {
            Error::new(
                literal.span(),
                format!("`{}` should look like name=description", pair),
            )
        })?;

        let name = name.trim();
        match declared
            .iter_mut()
            .find(|(declared, _, _)| declared.eq_ignore_ascii_case(name))
        {
            Some((_, slot, _)) => *slot = description.trim().to_string(),
            None => {
                return Err(Error::new(
                    literal.span(),
                    format!(
                        "`descriptions` names '{}', which is not in `arguments`",
                        name
                    ),
                ))
            }
        }
    }

    Ok(())
}

/// Expand `#[mcp_prompt(name = "...")] async fn render(args: Value) -> Result<Vec<McpPromptMessage>, _>`.
pub fn expand_mcp_prompt(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as PromptArgs);
    let input = parse_macro_input!(item as ItemFn);

    match build_prompt(args, input) {
        Ok(stream) => stream.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn build_prompt(args: PromptArgs, input: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let fn_ident = &input.sig.ident;
    let fn_name_str = LitStr::new(&fn_ident.to_string(), fn_ident.span());

    let name = args.name.clone().unwrap_or_else(|| fn_name_str.clone());
    let description = args.description.clone().unwrap_or_else(|| {
        LitStr::new(&format!("The '{}' prompt", name.value()), fn_ident.span())
    });

    let role_tokens = match &args.required_role {
        Some(role) => quote!(Some(#role)),
        None => quote!(None),
    };

    let declared: Vec<(String, String, bool)> = args.arguments.clone();
    let arguments_fn = build_arguments_fn(declared, fn_ident)?;

    let args_binding = single_binding(&input)?;
    let args_ty = single_binding_type(&input)?;

    // A payload of `serde_json::Value` is handed over untouched; any other type
    // is deserialized here, exactly as `#[mcp_tool]` does for its arguments.
    let args_expr = if is_json_value(&args_ty) {
        quote!(__gritshield_args)
    } else {
        quote! {
            ::gritshield::deps::serde_json::from_value::<#args_ty>(__gritshield_args)
                .map_err(|__error| {
                    ::gritshield::mcp::protocol::McpError::InvalidParams(
                        ::std::format!(
                            "invalid arguments for this prompt: {}",
                            __error
                        )
                    )
                })?
        }
    };

    let expanded = quote! {
        #input

        ::gritshield::inventory::submit! {
            ::gritshield::mcp::prompt::McpPromptRegistration {
                name: #name,
                description: #description,
                arguments: #arguments_fn,
                required_role: #role_tokens,
                builder: |__gritshield_args| ::std::boxed::Box::pin(async move {
                    let #args_binding = #args_expr;
                    #fn_ident(#args_binding)
                        .await
                        .map_err(::gritshield::mcp::protocol::McpError::from)
                }),
            }
        }
    };

    Ok(expanded)
}

/// Emit the const-constructible argument descriptor the registration needs.
fn build_arguments_fn(
    declared: Vec<(String, String, bool)>,
    span_ident: &syn::Ident,
) -> syn::Result<proc_macro2::TokenStream> {
    let span = span_ident.span();
    let entries: Vec<proc_macro2::TokenStream> = declared
        .into_iter()
        .map(|(name, description, required)| {
            let name = LitStr::new(&name, span);
            let description = LitStr::new(&description, span);
            let constructor = if required {
                quote!(required)
            } else {
                quote!(optional)
            };

            quote! {
                ::gritshield::mcp::prompt::McpPromptArgument::#constructor(#name, #description)
            }
        })
        .collect();

    Ok(quote! {
        || -> ::std::vec::Vec<::gritshield::mcp::prompt::McpPromptArgument> {
            ::std::vec![#(#entries),*]
        }
    })
}

/// Fetch a mandatory attribute value, or fail with an actionable message.
fn require<'a>(
    value: &'a Option<LitStr>,
    span: proc_macro2::Span,
    message: &str,
) -> syn::Result<&'a LitStr> {
    value
        .as_ref()
        .ok_or_else(|| Error::new(span, message))
}

fn count_inputs(function: &ItemFn) -> usize {
    function
        .sig
        .inputs
        .iter()
        .filter(|arg| matches!(arg, FnArg::Typed(_)))
        .count()
}

fn context_binding(function: &ItemFn) -> syn::Result<syn::Ident> {
    let inputs: Vec<&FnArg> = function
        .sig
        .inputs
        .iter()
        .filter(|arg| matches!(arg, FnArg::Typed(_)))
        .collect();

    match inputs.first() {
        Some(first) => match &**first {
            FnArg::Typed(typed) => match &*typed.pat {
                syn::Pat::Ident(ident) => Ok(ident.ident.clone()),
                other => Err(Error::new_spanned(other, "expected a named parameter")),
            },
            FnArg::Receiver(_) => Err(Error::new(
                fn_ident_span(function),
                "#[mcp_resource] handlers must be free functions, not methods",
            )),
        },
        None => Ok(syn::Ident::new("_gritshield_ctx", fn_ident_span(function))),
    }
}

fn single_binding(function: &ItemFn) -> syn::Result<syn::Ident> {
    let inputs: Vec<&FnArg> = function
        .sig
        .inputs
        .iter()
        .filter(|arg| matches!(arg, FnArg::Typed(_)))
        .collect();

    if inputs.len() != 1 {
        return Err(Error::new_spanned(
            &function.sig,
            "#[mcp_prompt] handlers take exactly one parameter: the JSON argument payload",
        ));
    }

    match inputs[0] {
        FnArg::Typed(typed) => match &*typed.pat {
            syn::Pat::Ident(ident) => Ok(ident.ident.clone()),
            other => Err(Error::new_spanned(other, "expected a named parameter")),
        },
        FnArg::Receiver(_) => Err(Error::new(
            fn_ident_span(function),
            "#[mcp_prompt] handlers must be free functions, not methods",
        )),
    }
}

fn fn_ident_span(function: &ItemFn) -> proc_macro2::Span {
    function.sig.ident.span()
}

/// The declared type of the prompt's single payload parameter.
fn single_binding_type(function: &ItemFn) -> syn::Result<syn::Type> {
    match function
        .sig
        .inputs
        .iter()
        .find(|arg| matches!(arg, FnArg::Typed(_)))
    {
        Some(FnArg::Typed(typed)) => Ok((*typed.ty).clone()),
        _ => Err(Error::new_spanned(
            &function.sig,
            "#[mcp_prompt] handlers take exactly one parameter: the JSON argument payload",
        )),
    }
}

/// Whether a type is `serde_json::Value`.
fn is_json_value(ty: &syn::Type) -> bool {
    quote!(#ty).to_string().replace(' ', "").ends_with("Value")
}