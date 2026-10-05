use crate::routing::engine::RequestContext;
use crate::security::xss::UntrustedString;
use futures_util::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_tungstenite::{tungstenite::Message, WebSocketStream};

pub type BoxedWsFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
pub type WsHandlerFn = fn(WebSocketStream<TcpStream>, RequestContext) -> BoxedWsFuture;

lazy_static::lazy_static! {
    pub static ref WS_REGISTRY: Mutex<HashMap<String, WsHandlerFn>> = Mutex::new(HashMap::new());
}

pub fn register_ws_route(path: &str, handler: WsHandlerFn) {
    if let Ok(mut map) = WS_REGISTRY.lock() {
        map.insert(path.to_string(), handler);
    }
}

pub trait WebSocketHandler: Send + Sync + 'static {
    type Message: Serialize + DeserializeOwned + Send + 'static;

    fn on_connect(&self, _ctx: &RequestContext) -> BoxedWsFuture {
        Box::pin(async {})
    }

    fn on_message(&self, msg: Self::Message, ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture;

    fn on_close(&self, _ctx: &RequestContext) -> BoxedWsFuture {
        Box::pin(async {})
    }

    fn on_error(&self, _err: WsError, _ctx: &RequestContext) -> BoxedWsFuture {
        Box::pin(async {})
    }
}

#[derive(Clone)]
pub struct WsSink {
    sender: tokio::sync::mpsc::UnboundedSender<WsOutgoing>,
}

impl WsSink {
    pub fn new(sender: tokio::sync::mpsc::UnboundedSender<WsOutgoing>) -> Self {
        Self { sender }
    }

    pub fn send<T: Serialize + Send + 'static>(&self, msg: &T) -> Result<(), WsError> {
        let json = serde_json::to_string(msg).map_err(WsError::Serialize)?;
        self.sender
            .send(WsOutgoing::Text(json))
            .map_err(|_| WsError::SendFailed)
    }

    pub fn send_raw(&self, msg: WsOutgoing) -> Result<(), WsError> {
        self.sender.send(msg).map_err(|_| WsError::SendFailed)
    }

    pub fn close(&self) -> Result<(), WsError> {
        self.sender.send(WsOutgoing::Close).map_err(|_| WsError::SendFailed)
    }

    pub fn ping(&self, data: Vec<u8>) -> Result<(), WsError> {
        self.sender.send(WsOutgoing::Ping(data)).map_err(|_| WsError::SendFailed)
    }
}

pub enum WsOutgoing {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
    InternalPong(Vec<u8>),
}

#[derive(Debug, Error)]
pub enum WsError {
    #[error("Serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("Send failed: channel closed")]
    SendFailed,
    #[error("WebSocket protocol error: {0}")]
    Protocol(String),
    #[error("Handler not found for path: {0}")]
    NotFound(String),
}

pub struct WsConnection<H: WebSocketHandler> {
    handler: H,
    ctx: RequestContext,
    sink: WsSink,
    rx: tokio::sync::mpsc::UnboundedReceiver<WsIncoming>,
    incoming_tx: tokio::sync::mpsc::UnboundedSender<WsIncoming>,
    out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<WsOutgoing>>,
}

impl<H: WebSocketHandler> WsConnection<H> {
    pub fn new(handler: H, ctx: RequestContext) -> (Self, WsSink) {
        let (incoming_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
        let sink = WsSink::new(out_tx);
        let conn = Self {
            handler,
            ctx,
            sink: sink.clone(),
            rx,
            incoming_tx,
            out_rx: Some(out_rx),
        };
        (conn, sink)
    }

    pub async fn run(mut self, mut ws_stream: WebSocketStream<TcpStream>) {
        self.handler.on_connect(&self.ctx).await;

        let (mut ws_tx, mut ws_rx) = ws_stream.split();

        let out_rx = self.out_rx.take().unwrap();
        let out_tx = self.sink.sender.clone();
        let sender_task = tokio::spawn(async move {
            let mut out_rx = out_rx;
            while let Some(outgoing) = out_rx.recv().await {
                let msg = match outgoing {
                    WsOutgoing::Text(t) => Message::Text(t.into()),
                    WsOutgoing::Binary(b) => Message::Binary(b.into()),
                    WsOutgoing::Ping(d) => Message::Ping(d.into()),
                    WsOutgoing::Pong(d) => Message::Pong(d.into()),
                    WsOutgoing::InternalPong(d) => Message::Pong(d.into()),
                    WsOutgoing::Close => Message::Close(None),
                };
                if ws_tx.send(msg).await.is_err() {
                    break;
                }
            }
        });

        let incoming_tx = self.incoming_tx.clone();
        while let Some(msg) = ws_rx.next().await {
            match msg {
                Ok(Message::Text(t)) => {
                    let _ = incoming_tx.send(WsIncoming::Message(t.to_string()));
                }
                Ok(Message::Binary(b)) => {
                    let _ = incoming_tx.send(WsIncoming::Message(String::from_utf8_lossy(&b).to_string()));
                }
                Ok(Message::Close(_)) => {
                    let _ = incoming_tx.send(WsIncoming::Close);
                    break;
                }
                Ok(Message::Ping(d)) => {
                    let _ = out_tx.send(WsOutgoing::InternalPong(d));
                }
                Ok(Message::Pong(_)) => {}
                Ok(Message::Frame(_)) => {}
                Err(e) => {
                    let _ = incoming_tx.send(WsIncoming::Error(e.to_string()));
                    break;
                }
            }
        }

        while let Some(incoming) = self.rx.recv().await {
            match incoming {
                WsIncoming::Message(msg) => {
                    match serde_json::from_str::<H::Message>(&msg) {
                        Ok(parsed) => {
                            self.handler.on_message(parsed, &self.ctx, self.sink.clone()).await;
                        }
                        Err(e) => {
                            self.handler.on_error(WsError::Serialize(e), &self.ctx).await;
                        }
                    }
                }
                WsIncoming::Close => break,
                WsIncoming::Error(e) => {
                    self.handler.on_error(WsError::Protocol(e), &self.ctx).await;
                    break;
                }
            }
        }

        self.handler.on_close(&self.ctx).await;
        let _ = sender_task.await;
    }
}

pub enum WsIncoming {
    Message(String),
    Close,
    Error(String),
}

pub fn extract_ws_params(path: &str, registered_path: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let reg_parts: Vec<&str> = registered_path.split('/').filter(|s| !s.is_empty()).collect();

    for (i, reg_part) in reg_parts.iter().enumerate() {
        if reg_part.starts_with(':') {
            let param_name = &reg_part[1..];
            if let Some(value) = path_parts.get(i) {
                params.insert(param_name.to_string(), value.to_string());
            }
        }
    }
    params
}

impl RequestContext {
    pub fn ws_param(&self, key: &str) -> Option<&UntrustedString> {
        self.params.get(key)
    }

    pub fn ws_params(&self) -> &HashMap<String, UntrustedString> {
        &self.params
    }
}

pub fn path_matches(request_path: &str, registered_path: &str) -> bool {
    let req_parts: Vec<&str> = request_path.split('/').filter(|s| !s.is_empty()).collect();
    let reg_parts: Vec<&str> = registered_path.split('/').filter(|s| !s.is_empty()).collect();

    if req_parts.len() != reg_parts.len() {
        return false;
    }

    for (req, reg) in req_parts.iter().zip(reg_parts.iter()) {
        if !reg.starts_with(':') && req != reg {
            return false;
        }
    }
    true
}