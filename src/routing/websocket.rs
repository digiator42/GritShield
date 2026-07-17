use crate::routing::engine::RequestContext;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;

pub type BoxedWsFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
// The framework hands the developer the upgraded stream and the original context
pub type WsHandlerFn = fn(WebSocketStream<TcpStream>, RequestContext) -> BoxedWsFuture;

lazy_static::lazy_static! {
    pub static ref WS_REGISTRY: Mutex<HashMap<String, WsHandlerFn>> = Mutex::new(HashMap::new());
}

/// Core function used by macros to register websocket paths
pub fn register_ws_route(path: &str, handler: WsHandlerFn) {
    if let Ok(mut map) = WS_REGISTRY.lock() {
        map.insert(path.to_string(), handler);
    }
}
