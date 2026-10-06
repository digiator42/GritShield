// Public logger macros
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::core::logger::get_logger().log(
            $crate::core::logger::LogLevel::Error,
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::core::logger::get_logger().log(
            $crate::core::logger::LogLevel::Warn,
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::core::logger::get_logger().log(
            $crate::core::logger::LogLevel::Info,
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::core::logger::get_logger().log(
            $crate::core::logger::LogLevel::Debug,
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::core::logger::get_logger().log(
            $crate::core::logger::LogLevel::Trace,
            format_args!($($arg)*)
        )
    };
}

// render macro
#[macro_export]
macro_rules! render {
    // Mode A: Standard Maud Markup Render (Async)
    ($ctx:expr, $title:expr, $markup:expr) => {{
        // Await the main_layout async function
        let final_html = crate::root::layout::main_layout($title, $markup, &$ctx)
            .await
            .into_string();

        $crate::http::response::Response::new(
            200,
            $crate::security::xss::Sanitizer::trust(&final_html),
        )
    }};

    // Mode B: Raw HTML String Injection via explicit token flag
    ($ctx:expr, $title:expr, raw $html_string:expr) => {{
        let raw_wrapper = maud::PreEscaped($html_string);
        let final_html = crate::root::layout::main_layout($title, raw_wrapper, &$ctx)
            .await
            .into_string();

        $crate::http::response::Response::new(
            200,
            $crate::security::xss::Sanitizer::trust(&final_html),
        )
    }};
}

// file system route macros

#[macro_export]
macro_rules! register_page {
    // Pattern 1: With explicit named role verification tracking (e.g., role = "Admin")
    ($method:expr, $handler:expr, role = $role:expr $(,)?) => {
        #[$crate::ctor::ctor(unsafe)]
        fn register_route() {
            let mut raw_file_path = file!().replace("\\", "/");

            // Normalize path to anchor from "src/" onwards to fix Cargo Workspace prefixes
            if let Some(src_idx) = raw_file_path.find("src/") {
                raw_file_path = raw_file_path[src_idx..].to_string();
            }

            if let Ok(mut registry) = $crate::routing::file_system::FILE_ROUTING_REGISTRY.lock() {
                registry.insert(
                    raw_file_path,
                    $crate::routing::file_system::RegisteredFileRoute {
                        method: $method,
                        handler_factory: || Box::new($handler),
                        required_role: Some($role), // Bound cleanly into runtime ledger
                    },
                );
            }
        }
    };

    // Pattern 2: Default fallback matching variant with no specified role constraint
    ($method:expr, $handler:expr $(,)?) => {
        #[$crate::ctor::ctor(unsafe)]
        fn register_route() {
            let mut raw_file_path = file!().replace("\\", "/");

            if let Some(src_idx) = raw_file_path.find("src/") {
                raw_file_path = raw_file_path[src_idx..].to_string();
            }

            if let Ok(mut registry) = $crate::routing::file_system::FILE_ROUTING_REGISTRY.lock() {
                registry.insert(
                    raw_file_path,
                    $crate::routing::file_system::RegisteredFileRoute {
                        method: $method,
                        handler_factory: || Box::new($handler),
                        required_role: None,
                    },
                );
            }
        }
    };
}

#[macro_export]
macro_rules! register_ws {
    ($path:expr, $handler:expr) => {
        #[$crate::ctor::ctor(unsafe)]
        fn __gritshield_ws_route_init() {
            // Registered through the generic form so the closure is boxed into a
// `WsHandlerFn` inside the framework. Naming the type here would hand
// `register_ws_route` an `Arc<dyn Fn..>`, which does not satisfy an `Fn` bound.
$crate::routing::websocket::register_ws_route($path, |stream, ctx| {
                Box::pin($handler(stream, ctx))
            });
        }
    };
}

/// Declares a [`WebSocketHandler`] impl from closures instead of a manual `impl`.
///
/// Every hook but `on_message` is optional. Each is emitted as "start from the
/// default, then overwrite it if the caller supplied one" — a single `$(...)?`
/// per method, which is what `macro_rules` actually accepts. (Chaining a second
/// `$(Box::pin(async {}))?` for the fallback, as this macro used to, is a
/// repetition with no metavariables and fails to expand at all.)
///
/// ```ignore
/// ws_handler!(
///     Echo,
///     message = String,
///     on_message = |msg: String, _ctx: &RequestContext, ws: WsSink| Box::pin(async move {
///         let _ = ws.send_text(msg).await;
///     }),
/// );
/// ```
#[macro_export]
macro_rules! ws_handler {
    (
        $name:ident,
        message = $msg_type:ty,
        $(on_connect = $on_connect:expr,)?
        $(on_message = $on_message:expr,)?
        $(on_close = $on_close:expr,)?
        $(on_error = $on_error:expr,)?
    ) => {
        struct $name;

        impl $crate::routing::websocket::WebSocketHandler for $name {
            type Message = $msg_type;

            fn on_connect(
                &self,
                ctx: &$crate::routing::engine::RequestContext,
                ws: &$crate::routing::websocket::WsSink,
            ) -> $crate::routing::websocket::BoxedWsFuture {
                #[allow(unused_mut, unused_variables)]
                let mut hook: $crate::routing::websocket::BoxedWsFuture =
                    $crate::routing::websocket::ws_noop(ctx, ws);
                $(
                    hook = Box::pin($on_connect(ctx, ws));
                )?
                hook
            }

            fn on_message(
                &self,
                msg: Self::Message,
                ctx: &$crate::routing::engine::RequestContext,
                ws: $crate::routing::websocket::WsSink,
            ) -> $crate::routing::websocket::BoxedWsFuture {
                #[allow(unused_mut, unused_variables)]
                let mut hook: $crate::routing::websocket::BoxedWsFuture =
                    $crate::routing::websocket::ws_noop_message(&msg, ctx, &ws);
                $(
                    hook = Box::pin($on_message(msg, ctx, ws));
                )?
                hook
            }

            fn on_close(
                &self,
                ctx: &$crate::routing::engine::RequestContext,
                ws: &$crate::routing::websocket::WsSink,
            ) -> $crate::routing::websocket::BoxedWsFuture {
                #[allow(unused_mut, unused_variables)]
                let mut hook: $crate::routing::websocket::BoxedWsFuture =
                    $crate::routing::websocket::ws_noop(ctx, ws);
                $(
                    hook = Box::pin($on_close(ctx, ws));
                )?
                hook
            }

            fn on_error(
                &self,
                err: $crate::routing::websocket::WsError,
                ctx: &$crate::routing::engine::RequestContext,
                ws: &$crate::routing::websocket::WsSink,
            ) -> $crate::routing::websocket::BoxedWsFuture {
                #[allow(unused_mut, unused_variables)]
                let mut hook: $crate::routing::websocket::BoxedWsFuture =
                    $crate::routing::websocket::ws_noop_err(&err, ctx, ws);
                $(
                    hook = Box::pin($on_error(err, ctx, ws));
                )?
                hook
            }
        }
    };
}

#[macro_export]
macro_rules! ws_message {
    ($name:ident { $( $field:ident: $type:ty ),* $(,)? }) => {
        #[derive(serde::Serialize, serde::Deserialize, Debug, Clone)]
        pub struct $name {
            $( pub $field: $type, )*
        }
    };
}

#[macro_export]
macro_rules! log_route {
    ($path:expr, $max_len:expr, $method:expr) => {
        crate::trace!(
            "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
            $path,
            $max_len,
            "->".green(),
            method_color($method)
        );
    };
}

#[macro_export]
macro_rules! declare_security_caps {
    ($( $cap:ty => [ $( $role:ty ),* ] ),* $(,)?) => {

        $(
            impl ::gritshield::security::rbac::GritSecurityCheck for $cap {}
            
            impl ::gritshield::security::rbac::GritCapabilityRuntime for $cap {
                fn name() -> &'static str {
                    std::stringify!($cap)
                }
                fn allowed_roles() -> &'static [&'static str] {
                    &[ $( std::stringify!($role) ),* ]
                }
            }

            // Automatically submit this layout metadata to our dashboard registry!
            gritshield::inventory::submit! {
                $crate::routing::engine::route::CapabilityRegistration {
                    name: std::stringify!($cap),
                    allowed_roles: &[ $( std::stringify!($role) ),* ],
                }
            }
        )*
    };
}

/// Pre-registers a mock instance for type `$ty` into a `GritContainer` or global `CONTEXT`.
/// Overrides lazy factory instantiation during integration tests.
#[macro_export]
macro_rules! mock {
    // Target a specific container instance
    ($container:expr, $ty:ty, $mock_instance:expr) => {
        $container.register::<$ty>($mock_instance);
    };
    // Default to global CONTEXT
    ($ty:ty, $mock_instance:expr) => {
        $crate::core::ioc::CONTEXT.register::<$ty>($mock_instance);
    };
}
