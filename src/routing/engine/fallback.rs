use crate::http::response::Response;
use super::context::RequestContext;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use lazy_static::lazy_static;

pub type AsyncPageFuture = Pin<Box<dyn Future<Output = Response> + Send>>;
pub type PageHandlerFn = fn(RequestContext) -> AsyncPageFuture;

lazy_static! {
    pub static ref GLOBAL_FALLBACK: Mutex<Option<PageHandlerFn>> = Mutex::new(None);
}

pub fn register_global_fallback(handler: PageHandlerFn) {
    if let Ok(mut guard) = GLOBAL_FALLBACK.lock() {
        *guard = Some(handler);
    }
}