use proc_macro::TokenStream;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    parse_macro_input, Error, FnArg, ItemFn, LitBool, LitStr, Pat, ReturnType, Token, Type,
};

/// Arguments accepted by `#[mcp_tool(...)]`.
///
/// ```ignore
/// #[mcp_tool(
///     name = "search_users",
///     description = "Search users by email",
///     schema = { "type": "object", "properties": { "q": { "type": "string" } } },
///     required_role = "Admin",
///     enabled = true,
///     service = "UserService"
/// )]
/// ```
#[derive(Default)]
pub struct ToolArgs {
    pub name: Option<LitStr>,
    pub description: Option<LitStr>,
    /// The raw JSON text of the input schema.
    pub schema: Option<String>,
    pub required_role: Option<LitStr>,
    pub enabled: Option<bool>,
    pub service: Option<LitStr>,
}

impl Parse for ToolArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut args = ToolArgs::default();

        while !input.is_empty() {
            let ident = input.parse::<syn::Ident>()?;
            input.parse::<Token![=]>()?;

            match ident.to_string().as_str() {
                "name" => args.name = Some(input.parse()?),
                "description" => args.description = Some(input.parse()?),
                "required_role" => args.required_role = Some(input.parse()?),
                "service" => args.service = Some(input.parse()?),
                "enabled" => {
                    let literal: LitBool = input.parse()?;
                    args.enabled = Some(literal.value());
                }
                // The schema is an arbitrary JSON document, so it is validated
                // and captured as text.
                "schema" => args.schema = Some(parse_schema(input)?),
                other => {
                    return Err(Error::new(
                        ident.span(),
                        format!(
                            "Unknown argument `{}` for #[mcp_tool]. Expected one of: \
                             name, description, schema, required_role, enabled, service",
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

/// Capture the schema, which must be given as a JSON string literal.
///
/// ```ignore
/// schema = r#"{"type": "object", "properties": {"q": {"type": "string"}}}"#
/// ```
///
/// A bare `{ .. }` block cannot be used here: rustc parses every attribute's
/// arguments as a *meta item* before the macro ever runs, and a quoted key like
/// `"type":` is not valid meta syntax. Validating the JSON at expansion time
/// also turns a malformed schema into a compile error pointing at the schema
/// rather than a panic at server startup.
fn parse_schema(input: ParseStream) -> syn::Result<String> {
    let literal: LitStr = input.parse().map_err(|_| {
        Error::new(
            input.span(),
            "expected a JSON object in a string literal, e.g. \
             schema = r#\"{\"type\": \"object\"}\"#",
        )
    })?;

    let text = literal.value();

    match serde_json::from_str::<serde_json::Value>(&text) {
        Ok(value) if value.is_object() => Ok(text),
        Ok(_) => Err(Error::new(
            literal.span(),
            "the `schema` of #[mcp_tool] must be a JSON *object*",
        )),
        Err(error) => Err(Error::new(
            literal.span(),
            format!("the `schema` of #[mcp_tool] is not valid JSON: {}", error),
        )),
    }
}

pub fn expand_mcp_tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ToolArgs);
    let input = parse_macro_input!(item as ItemFn);

    match build_tool(args, input) {
        Ok(stream) => stream.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn build_tool(args: ToolArgs, input: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let fn_ident = &input.sig.ident;
    let fn_name_str = LitStr::new(&fn_ident.to_string(), fn_ident.span());

    let name = args.name.clone().unwrap_or_else(|| fn_name_str.clone());
    let description = args.description.clone().unwrap_or_else(|| {
        LitStr::new(
            &format!("The '{}' tool", name.value()),
            fn_ident.span(),
        )
    });
    let service = args.service.clone().unwrap_or_else(|| LitStr::new("", fn_ident.span()));

    let enabled = LitBool::new(args.enabled.unwrap_or(true), fn_ident.span());

    let role_tokens = match &args.required_role {
        Some(role) => quote!(Some(#role)),
        None => quote!(None),
    };

    let schema_tokens = match &args.schema {
        Some(json) => {
            // Already proven to be a JSON object during expansion.
            let json = LitStr::new(json, fn_ident.span());
            quote!(|| -> ::gritshield::deps::serde_json::Value {
                ::gritshield::deps::serde_json::from_str(#json)
                    .expect("#[mcp_tool] schema validated at compile time")
            })
        }
        None => quote!(|| -> ::gritshield::deps::serde_json::Value {
            ::gritshield::mcp::schema::empty_object_schema()
        }),
    };

    let signature = ToolSignature::parse(&input)?;

    let call = signature.build_call(fn_ident)?;

    // The handler must be a non-capturing closure so it coerces to the
    // `McpToolFn` function pointer that the const inventory entry needs.
    let expanded = quote! {
        #input

        ::gritshield::inventory::submit! {
            ::gritshield::mcp::tool::McpToolRegistration {
                name: #name,
                description: #description,
                service: #service,
                input_schema: #schema_tokens,
                required_role: #role_tokens,
                enabled_by_default: #enabled,
                handler: |__gritshield_ctx, __gritshield_args| ::std::boxed::Box::pin(async move {
                    #call
                }),
            }
        }
    };

    Ok(expanded)
}

/// The parsed parameter shape of a tool handler.
///
/// Two conventions are supported, and both are checked at compile time rather
/// than failing with an inscrutable type error later:
///
/// ```ignore
/// async fn tool(ctx: RequestContext, args: SearchArgs) -> Result<..>   // 2 params
/// async fn tool(ctx: RequestContext, args: SearchArgs, db: Arc<Db>)   // trailing DI
/// ```
struct ToolSignature {
    /// How the JSON payload is turned into the handler's second parameter.
    args_mode: ArgsMode,
    /// The concrete type of the argument parameter.
    args_type: Type,
    /// Leading non-context parameters injected from the DI container.
    di_types: Vec<Type>,
    ctx_binding: syn::Ident,
    args_binding: syn::Ident,
    /// The handler asked for `&RequestContext` rather than `RequestContext`.
    ctx_by_reference: bool,
    /// The `Ok(..)` payload is serialized to JSON unless it already is.
    return_mode: ReturnMode,
}

enum ArgsMode {
    /// The second parameter is `serde_json::Value` and is passed through.
    Raw,
    /// The second parameter is a concrete type deserialized from JSON.
    Typed,
}

enum ReturnMode {
    /// Return `Result<Value, _>`; the payload is already JSON.
    Raw,
    /// Return `Result<T, _>`; the payload is serialized with `to_value`.
    Typed,
}

impl ToolSignature {
    fn parse(function: &ItemFn) -> syn::Result<Self> {
        let inputs: Vec<&FnArg> = function
            .sig
            .inputs
            .iter()
            .filter(|arg| matches!(arg, FnArg::Typed(_)))
            .collect();

        if inputs.len() < 2 {
            return Err(Error::new_spanned(
                &function.sig,
                "#[mcp_tool] handlers must take (ctx: RequestContext, args: ..) — \
                 the request context and a JSON argument payload",
            ));
        }

        let ctx_binding = binding_ident(inputs[0])?;
        if !is_request_context(inputs[0]) {
            return Err(Error::new_spanned(
                inputs[0],
                "the first parameter of an #[mcp_tool] handler must be \
                 `gritshield::routing::engine::RequestContext`",
            ));
        }

        let args_binding = binding_ident(inputs[1])?;
        let args_type = typed(inputs[1])?.clone();
        let args_mode = if is_json_value(&args_type) {
            ArgsMode::Raw
        } else {
            ArgsMode::Typed
        };

        let di_types = inputs[2..]
            .iter()
            .map(|arg| typed(arg).cloned())
            .collect::<syn::Result<Vec<_>>>()?;

        let return_mode = match output_type(function) {
            Some(ty) if is_json_value(&ty) => ReturnMode::Raw,
            Some(_) => ReturnMode::Typed,
            None => {
                return Err(Error::new_spanned(
                    &function.sig.output,
                    "#[mcp_tool] handlers must return a Result",
                ))
            }
        };

        Ok(Self {
            args_mode,
            args_type,
            di_types,
            ctx_binding,
            args_binding,
            ctx_by_reference: takes_context_reference(inputs[0]),
            return_mode,
        })
    }

    /// Build the body of the boxed future that `inventory` stores.
    fn build_call(&self, callee: &syn::Ident) -> syn::Result<proc_macro2::TokenStream> {
        let ctx = &self.ctx_binding;
        let args = &self.args_binding;

        let args_type = &self.args_type;
        let ctx_expr = if self.ctx_by_reference {
            quote!(&__gritshield_ctx)
        } else {
            quote!(__gritshield_ctx)
        };

        let args_expr = match self.args_mode {
            // Both arms read from the closure parameter; the handler's own
            // parameter names are rebound below so the call site reads exactly
            // as the developer wrote it.
            ArgsMode::Raw => quote!(__gritshield_args),
            // The target type is named explicitly rather than inferred, so the
            // deserialization can never fall back to `()`.
            ArgsMode::Typed => quote! {
                ::gritshield::deps::serde_json::from_value::<#args_type>(__gritshield_args)
                    .map_err(|__error| {
                        ::gritshield::mcp::protocol::McpError::InvalidParams(
                            ::std::format!(
                                "invalid arguments for this tool: {}",
                                __error
                            )
                        )
                    })?
            },
        };

        let mut trailing = Vec::new();
        for ty in &self.di_types {
            trailing.push(quote! {
                ::gritshield::core::ioc::CONTEXT.get::<#ty>().map_err(
                    ::gritshield::mcp::protocol::McpError::Internal
                )?
            });
        }

        // `Result<Value, _>` yields JSON directly; any other payload type is
        // serialized. Both go through `map_err` so a handler may return either
        // `String` or `McpError` as its failure type.
        let serialize = match self.return_mode {
            ReturnMode::Raw => quote!(__ok),
            ReturnMode::Typed => quote! {
                ::gritshield::deps::serde_json::to_value(__ok).map_err(|__error| {
                    ::gritshield::mcp::protocol::McpError::Internal(
                        ::std::format!(
                            "the tool returned a value that is not JSON-serializable: {}",
                            __error
                        )
                    )
                })?
            },
        };

        Ok(quote! {
            // Borrow or move the context to match however the handler asked for
            // it, so `ctx: RequestContext` and `ctx: &RequestContext` both work.
            let #ctx = #ctx_expr;
            let #args = #args_expr;
            #callee(#ctx, #args #(, #trailing)*)
                .await
                .map_err(::gritshield::mcp::protocol::McpError::from)
                .and_then(|__ok| -> ::std::result::Result<::gritshield::deps::serde_json::Value, ::gritshield::mcp::protocol::McpError> {
                    Ok(#serialize)
                })
        })
    }
}

fn binding_ident(arg: &FnArg) -> syn::Result<syn::Ident> {
    match arg {
        FnArg::Typed(typed) => match &*typed.pat {
            Pat::Ident(ident) => Ok(ident.ident.clone()),
            other => Err(Error::new_spanned(other, "expected a named parameter")),
        },
        FnArg::Receiver(_) => Err(Error::new(
            proc_macro2::Span::call_site(),
            "#[mcp_tool] handlers must be free functions, not methods",
        )),
    }
}

fn typed(arg: &FnArg) -> syn::Result<&Type> {
    match arg {
        FnArg::Typed(typed) => Ok(&typed.ty),
        FnArg::Receiver(_) => Err(Error::new(
            proc_macro2::Span::call_site(),
            "#[mcp_tool] handlers must be free functions, not methods",
        )),
    }
}

fn is_request_context(arg: &FnArg) -> bool {
    let ty = match typed(arg) {
        Ok(ty) => ty,
        Err(_) => return false,
    };

    let rendered = quote!(#ty).to_string();
    rendered.ends_with("RequestContext")
}

/// Whether the handler wants the context by reference.
///
/// Both forms are supported and both are idiomatic — a handler that only reads
/// the context should not have to take ownership of it. Mirrors the behaviour
/// of `#[mcp_resource]`.
fn takes_context_reference(arg: &FnArg) -> bool {
    matches!(typed(arg), Ok(Type::Reference(_)))
}

fn is_json_value(ty: &Type) -> bool {
    quote!(#ty).to_string().replace(' ', "").ends_with("Value")
}

/// The `Ok(..)` type inside `Result<_, _>`.
fn output_type(function: &ItemFn) -> Option<Type> {
    let return_type = match &function.sig.output {
        ReturnType::Default => return None,
        ReturnType::Type(_, ty) => ty,
    };

    if let Type::Path(path) = &**return_type {
        let segment = path.path.segments.last()?;
        if segment.ident != "Result" {
            return None;
        }
        let arguments = match &segment.arguments {
            syn::PathArguments::AngleBracketed(args) => args,
            _ => return None,
        };
        return arguments.args.iter().find_map(|arg| match arg {
            syn::GenericArgument::Type(ty) => Some(ty.clone()),
            _ => None,
        });
    }

    None
}