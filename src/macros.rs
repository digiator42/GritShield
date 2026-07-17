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
macro_rules! register_fallback_page {
    ($handler:expr) => {
        #[$crate::ctor::ctor(unsafe)]
        fn init_framework_fallback() {
            // Wrap the developer's async handler into a clean, pinned BoxFuture pointer
            let wrapped_handler: $crate::routing::engine::PageHandlerFn =
                |ctx| Box::pin($handler(ctx));

            $crate::routing::engine::register_global_fallback(wrapped_handler);
        }
    };
}

#[macro_export]
macro_rules! register_ws {
    ($path:expr, $handler:expr) => {
        #[$crate::ctor::ctor(unsafe)]
        fn init_ws_route() {
            let wrapped: $crate::routing::websocket::WsHandlerFn =
                |stream, ctx| Box::pin($handler(stream, ctx));
            $crate::routing::websocket::register_ws_route($path, wrapped);
        }
    };
}
