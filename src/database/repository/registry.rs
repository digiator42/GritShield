use crate::routing::trie::RequestContext;
use crate::http::response::Response;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use crate::deps::once_cell::sync::Lazy;

// Type alias for the type-erased admin dashboard request handlers
pub type AdminHandlerFn =
    Arc<dyn Fn(RequestContext) -> Pin<Box<dyn Future<Output = Response> + Send>> + Send + Sync>;

/// A custom action that can be executed on one or more records.
pub struct CustomAction {
    pub label: &'static str,
    pub icon: Option<&'static str>,
    pub color: &'static str, // CSS color class (e.g., "text-emerald-400", "text-amber-400")
    pub action: AdminHandlerFn,
}

/// Registry for custom actions (per table)
pub type ActionRegistry = HashMap<&'static str, Vec<Arc<CustomAction>>>;

// Global action registry
pub static ACTIONS_REGISTRY: Lazy<Mutex<ActionRegistry>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Register a custom action for a specific table
pub fn register_action(table_slug: &'static str, action: CustomAction) {
    if let Ok(mut registry) = ACTIONS_REGISTRY.lock() {
        registry
            .entry(table_slug)
            .or_insert_with(Vec::new)
            .push(Arc::new(action));
    }
}

#[derive(Clone)]
pub struct ModelMetadata {
    pub table_name: &'static str,
    pub table_slug: &'static str,
    pub route_path: &'static str,
    pub searchable_columns: Vec<&'static str>,
    pub list_handler: AdminHandlerFn,
    pub search_handler: AdminHandlerFn,
    pub delete_handler: AdminHandlerFn,
    pub patch_handler: AdminHandlerFn,
    pub advanced_search_handler: AdminHandlerFn,
    pub detail_handler: AdminHandlerFn,
    pub bulk_delete_handler: AdminHandlerFn,
    pub bulk_create_records_handler: AdminHandlerFn,
    pub bulk_create_modal_handler: AdminHandlerFn,
    pub export_handler: AdminHandlerFn,
}

pub trait AdminFieldParser {
    fn parse_field(s: &str) -> Result<Self, sea_orm::DbErr>
    where
        Self: Sized;
}

// Implement for standard strings
impl AdminFieldParser for String {
    fn parse_field(s: &str) -> Result<Self, sea_orm::DbErr> {
        Ok(s.to_string())
    }
}

// Implement for primitive scalar types
macro_rules! impl_primitive_parser {
    ($($t:ty),*) => {
        $(
            impl AdminFieldParser for $t {
                fn parse_field(s: &str) -> Result<Self, sea_orm::DbErr> {
                    s.parse::<Self>().map_err(|e| sea_orm::DbErr::Custom(format!("Parse error: {}", e)))
                }
            }
        )*
    };
}
impl_primitive_parser!(i8, i16, i32, i64, u8, u16, u32, u64, bool, f32, f64);



pub static ADMIN_REGISTRY: Lazy<Mutex<HashMap<&'static str, ModelMetadata>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn register_model(table: &'static str, meta: ModelMetadata) {
    if let Ok(mut registry) = ADMIN_REGISTRY.lock() {
        registry.insert(table, meta);
    }
}