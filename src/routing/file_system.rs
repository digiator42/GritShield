use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Mutex;

use crate::protocol::request::HttpMethod;
use crate::routing::trie::IntoHandler;

pub struct RegisteredFileRoute {
    pub method: HttpMethod,
    // We store a factory function pointer that generates our boxed handler trait object
    pub handler_factory: fn() -> Box<dyn IntoHandler>,
}

pub static FILE_ROUTING_REGISTRY: Lazy<Mutex<HashMap<String, RegisteredFileRoute>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[macro_export]
macro_rules! register_page {
    ($method:expr, $handler:expr) => {
        #[$crate::ctor::ctor(unsafe)]
        fn register_route() {
            // file!() to guarantee the key perfectly matches the filesystem path!
            let raw_file_path = file!().replace("\\", "/");

            if let Ok(mut registry) = $crate::routing::file_system::FILE_ROUTING_REGISTRY.lock() {
                registry.insert(
                    raw_file_path,
                    $crate::routing::file_system::RegisteredFileRoute {
                        method: $method,
                        handler_factory: || Box::new($handler),
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
            let wrapped_handler: $crate::routing::trie::PageHandlerFn =
                |ctx| Box::pin($handler(ctx));

            $crate::routing::trie::register_global_fallback(wrapped_handler);
        }
    };
}

#[macro_export]
macro_rules! register_ws {
    ($path:expr, $handler:expr) => {
        #[$crate::ctor::ctor(unsafe)]
        fn init_ws_route() {
            let wrapped: $crate::routing::websocket::WsHandlerFn = |stream, ctx| {
                Box::pin($handler(stream, ctx))
            };
            $crate::routing::websocket::register_ws_route($path, wrapped);
        }
    };
}