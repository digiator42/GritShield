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

// In src/routing/file_system.rs
#[macro_export]
macro_rules! register_page {
    ($method:expr, $handler:expr) => {
        #[$crate::ctor::ctor(unsafe)] 
        fn register_route() {
            // file!() to guarantee the key perfectly matches the filesystem path!
            let raw_file_path = file!().replace("\\", "/"); 
            
            if let Ok(mut registry) = $crate::routing::file_system::FILE_ROUTING_REGISTRY.lock() {
                registry.insert(raw_file_path, $crate::routing::file_system::RegisteredFileRoute {
                    method: $method,
                    handler_factory: || Box::new($handler),
                });
            }
        }
    };
}