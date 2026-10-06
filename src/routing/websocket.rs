//! WebSocket transport: route resolution, per-connection plumbing, and the
//! handler trait that application code implements.

use crate::routing::engine::RequestContext;
use crate::security::xss::UntrustedString;
use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use serde::{de::DeserializeOwned, Serialize};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    tungstenite::protocol::{frame::coding::CloseCode, CloseFrame},
    tungstenite::Message,
    WebSocketStream,
};

/// Return type of every [`WebSocketHandler`] hook.
pub type BoxedWsFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

/// A registered route's handler. Boxed so routes can capture state; plain `fn`
/// items and non-capturing closures coerce into it.
pub type WsHandlerFn =
    Arc<dyn Fn(WebSocketStream<TcpStream>, RequestContext) -> BoxedWsFuture + Send + Sync>;

/// Frames buffered per connection before the read loop starts applying TCP
/// backpressure to the peer.
pub const INCOMING_QUEUE_CAPACITY: usize = 64;

/// Outbound data frames buffered per connection before [`WsSink::send`] reports
/// [`WsError::Backpressure`].
pub const OUTGOING_QUEUE_CAPACITY: usize = 256;

/// Slots reserved for control frames (`ping`, `close`), which must never be
/// dropped behind application data.
pub const CONTROL_QUEUE_CAPACITY: usize = 8;

/// The only `Sec-WebSocket-Version` this server speaks.
pub const SUPPORTED_WS_VERSION: &str = "13";

/// How long teardown waits for a task to flush before reclaiming it.
const SHUTDOWN_FLUSH_TIMEOUT: Duration = Duration::from_millis(500);

// ---------------------------------------------------------------------------
// Route registry
// ---------------------------------------------------------------------------

/// One registered WebSocket route.
#[derive(Clone)]
pub struct WsRoute {
    pub handler: WsHandlerFn,
    /// Subprotocols the route accepts, in server preference order.
    pub subprotocols: Vec<String>,
}

/// A resolved route plus everything the upgrade needs to complete.
pub struct WsRouteMatch {
    pub handler: WsHandlerFn,
    pub params: HashMap<String, UntrustedString>,
    /// Echoed back in `Sec-WebSocket-Protocol` when the client offered one the
    /// route accepts.
    pub subprotocol: Option<String>,
}

lazy_static::lazy_static! {
    static ref WS_REGISTRY: RwLock<BTreeMap<String, WsRoute>> = RwLock::new(BTreeMap::new());
}

/// A poisoned registry still holds valid routes — a panic while holding the lock
/// must not silently unregister every WebSocket endpoint.
fn write_registry() -> RwLockWriteGuard<'static, BTreeMap<String, WsRoute>> {
    WS_REGISTRY
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn read_registry() -> RwLockReadGuard<'static, BTreeMap<String, WsRoute>> {
    WS_REGISTRY
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Registers `path` as a WebSocket endpoint. Overwrites any previous route for
/// the same path.
pub fn register_ws_route<F>(path: &str, handler: F)
where
    F: Fn(WebSocketStream<TcpStream>, RequestContext) -> BoxedWsFuture + Send + Sync + 'static,
{
    register_ws_route_with_subprotocols(path, &[], handler);
}

/// Like [`register_ws_route`], but negotiates a `Sec-WebSocket-Protocol` from
/// `subprotocols` (server preference order) when the client offers one of them.
pub fn register_ws_route_with_subprotocols<F>(
    path: &str,
    subprotocols: &[&str],
    handler: F,
) where
    F: Fn(WebSocketStream<TcpStream>, RequestContext) -> BoxedWsFuture + Send + Sync + 'static,
{
    write_registry().insert(
        path.to_string(),
        WsRoute {
            handler: Arc::new(handler),
            subprotocols: subprotocols.iter().map(|s| s.to_string()).collect(),
        },
    );
}

/// Every registered path, sorted. Useful for diagnostics and route listings.
pub fn registered_ws_paths() -> Vec<String> {
    read_registry().keys().cloned().collect()
}

/// Resolves the WebSocket route for `path`.
///
/// Matching mirrors the HTTP router: a static segment always beats a `:param`
/// at the same position, so `/ws/room/general` wins over `/ws/room/:room`
/// regardless of registration order. `offered_subprotocols` is the client's
/// `Sec-WebSocket-Protocol` list, most preferred first.
pub fn match_ws_route(path: &str, offered_subprotocols: &[String]) -> Option<WsRouteMatch> {
    let registry = read_registry();

    // `BTreeMap` iterates in path order, so keeping the first candidate at a
    // given specificity makes the outcome independent of registration order.
    let mut best: Option<(usize, &String, &WsRoute)> = None;

    for (registered, route) in registry.iter() {
        if !path_matches(path, registered) {
            continue;
        }
        let specificity = static_segments(registered);
        match &best {
            Some((best_specificity, _, _)) if *best_specificity >= specificity => {}
            _ => best = Some((specificity, registered, route)),
        }
    }

    let (_, registered, route) = best?;

    let subprotocol = route.subprotocols.iter().find_map(|accepted| {
        offered_subprotocols
            .iter()
            .find(|offered| offered.as_str() == accepted.as_str())
            .cloned()
    });

    Some(WsRouteMatch {
        handler: route.handler.clone(),
        params: extract_ws_params(path, registered),
        subprotocol,
    })
}

/// Whether any registered route matches `path`, regardless of subprotocols.
/// Used to answer a plain `GET` to a WebSocket path with `426` instead of a
/// confusing `404`.
pub fn has_ws_route(path: &str) -> bool {
    read_registry().keys().any(|registered| path_matches(path, registered))
}

/// Counts the non-parameter segments of a registered path; higher wins.
fn static_segments(registered: &str) -> usize {
    registered
        .split('/')
        .filter(|s| !s.is_empty() && !s.starts_with(':'))
        .count()
}

// ---------------------------------------------------------------------------
// Handler trait
// ---------------------------------------------------------------------------

pub trait WebSocketHandler: Send + Sync + 'static {
    type Message: Serialize + DeserializeOwned + Send + 'static;

    fn on_connect(&self, _ctx: &RequestContext, _ws: &WsSink) -> BoxedWsFuture {
        Box::pin(async {})
    }

    /// Runs sequentially per connection, so messages are handled in the order
    /// they arrived. The read loop stays responsive regardless: frames queue
    /// independently and only fill up once the handler falls behind.
    fn on_message(&self, msg: Self::Message, ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture;

    fn on_close(&self, _ctx: &RequestContext, _ws: &WsSink) -> BoxedWsFuture {
        Box::pin(async {})
    }

    fn on_error(&self, _err: WsError, _ctx: &RequestContext, _ws: &WsSink) -> BoxedWsFuture {
        Box::pin(async {})
    }
}

/// Lets a handler live behind an [`Arc`] — the only way to share one handler
/// across many connections, and the form `register_ws_route` takes.
impl<H: WebSocketHandler + ?Sized> WebSocketHandler for Arc<H> {
    type Message = H::Message;

    fn on_connect(&self, ctx: &RequestContext, ws: &WsSink) -> BoxedWsFuture {
        (**self).on_connect(ctx, ws)
    }

    fn on_message(&self, msg: Self::Message, ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
        (**self).on_message(msg, ctx, ws)
    }

    fn on_close(&self, ctx: &RequestContext, ws: &WsSink) -> BoxedWsFuture {
        (**self).on_close(ctx, ws)
    }

    fn on_error(&self, err: WsError, ctx: &RequestContext, ws: &WsSink) -> BoxedWsFuture {
        (**self).on_error(err, ctx, ws)
    }
}

/// The default `on_connect` / `on_close` body, used by `ws_handler!` when the
/// caller leaves a hook out.
///
/// A plain `fn` rather than an inline `async {}` in the macro: `macro_rules`
/// cannot emit "this expression *or* nothing" — a repetition has to contain a
/// metavariable — so each hook has to start from a fixed default it can
/// overwrite.
#[doc(hidden)]
pub fn ws_noop(_ctx: &RequestContext, _ws: &WsSink) -> BoxedWsFuture {
    Box::pin(async {})
}

/// The default `on_error` body, used by `ws_handler!`.
#[doc(hidden)]
pub fn ws_noop_err(_err: &WsError, _ctx: &RequestContext, _ws: &WsSink) -> BoxedWsFuture {
    Box::pin(async {})
}

/// The default `on_message` body, used by `ws_handler!`.
///
/// Takes the message and sink by reference so the caller can still move them
/// into a real hook: an inline `Box::pin(async { let _ = &msg; })` would borrow
/// the message for the rest of the method, and a by-value default would consume
/// the sink the hook needs.
#[doc(hidden)]
pub fn ws_noop_message<T: Send>(
    _msg: &T,
    _ctx: &RequestContext,
    _ws: &WsSink,
) -> BoxedWsFuture {
    Box::pin(async {})
}

// ---------------------------------------------------------------------------
// Sink
// ---------------------------------------------------------------------------

static NEXT_CONN_ID: AtomicUsize = AtomicUsize::new(0);

/// Write half of a connection. Cheap to clone; a clone kept in a registry lets
/// the connection be addressed for as long as it lives.
#[derive(Clone)]
pub struct WsSink {
    id: usize,
    data: tokio::sync::mpsc::Sender<WsOutgoing>,
    ctrl: tokio::sync::mpsc::Sender<WsOutgoing>,
}

impl WsSink {
    pub(crate) fn new(
        data: tokio::sync::mpsc::Sender<WsOutgoing>,
        ctrl: tokio::sync::mpsc::Sender<WsOutgoing>,
    ) -> Self {
        Self {
            id: NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed),
            data,
            ctrl,
        }
    }

    /// Stable identity of the connection. Unique per `WsConnection`, so it is
    /// safe as a registry key: register on `on_connect`, drop on `on_close`.
    pub fn id(&self) -> usize {
        self.id
    }

    /// Whether the connection is still able to accept frames.
    pub fn is_open(&self) -> bool {
        !self.data.is_closed()
    }

    /// Serializes `msg` to JSON and queues it as a text frame.
    ///
    /// Never blocks. A peer that cannot keep up yields
    /// [`WsError::Backpressure`] rather than growing the queue without bound.
    pub fn send<T: Serialize + Send + 'static>(&self, msg: &T) -> Result<(), WsError> {
        let json = serde_json::to_string(msg).map_err(WsError::Serialize)?;
        self.send_raw(WsOutgoing::Text(json))
    }

    /// Queues an already-serialized text frame.
    pub fn send_text(&self, text: impl Into<String>) -> Result<(), WsError> {
        self.send_raw(WsOutgoing::Text(text.into()))
    }

    /// Queues a binary frame.
    pub fn send_binary(&self, bytes: impl Into<Vec<u8>>) -> Result<(), WsError> {
        self.send_raw(WsOutgoing::Binary(bytes.into()))
    }

    /// Queues any frame. Control frames sent here travel on the control queue;
    /// everything else is subject to backpressure.
    pub fn send_raw(&self, msg: WsOutgoing) -> Result<(), WsError> {
        let target = match msg {
            WsOutgoing::Ping(_) | WsOutgoing::Close(_) => &self.ctrl,
            _ => &self.data,
        };
        target.try_send(msg).map_err(|err| match err {
            tokio::sync::mpsc::error::TrySendError::Full(_) => WsError::Backpressure,
            tokio::sync::mpsc::error::TrySendError::Closed(_) => WsError::SendFailed,
        })
    }

    /// Queues a close frame with no status code.
    pub fn close(&self) -> Result<(), WsError> {
        self.send_raw(WsOutgoing::Close(None))
    }

    /// Queues a close frame carrying a status code and reason.
    ///
    /// Codes the RFC forbids on the wire (1005, 1006, 1015, anything below
    /// 1000) are rejected with [`WsError::InvalidCloseCode`] rather than being
    /// put on the socket.
    pub fn close_with(&self, code: u16, reason: impl Into<String>) -> Result<(), WsError> {
        if !is_sendable_close_code(code) {
            return Err(WsError::InvalidCloseCode(code));
        }
        self.send_raw(WsOutgoing::Close(Some(WsClose {
            code,
            reason: reason.into(),
        })))
    }

    /// Queues a ping frame. The matching pong is answered by the transport.
    pub fn ping(&self, data: impl Into<Vec<u8>>) -> Result<(), WsError> {
        self.send_raw(WsOutgoing::Ping(data.into()))
    }
}

/// Status code and reason for a close handshake.
#[derive(Debug, Clone)]
pub struct WsClose {
    pub code: u16,
    pub reason: String,
}

pub enum WsOutgoing {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<WsClose>),
    InternalPong(Vec<u8>),
}

impl WsOutgoing {
    fn is_terminal(&self) -> bool {
        matches!(self, WsOutgoing::Close(_))
    }
}

#[derive(Debug, Error)]
pub enum WsError {
    #[error("Serialization failed: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("Send failed: channel closed")]
    SendFailed,
    #[error("Send failed: peer is not draining its queue")]
    Backpressure,
    #[error("Close code {0} may not be sent on the wire")]
    InvalidCloseCode(u16),
    #[error("WebSocket protocol error: {0}")]
    Protocol(String),
    #[error("Handler not found for path: {0}")]
    NotFound(String),
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

pub struct WsConnection<H: WebSocketHandler> {
    handler: Arc<H>,
    ctx: Arc<RequestContext>,
    sink: WsSink,
    data_rx: Option<tokio::sync::mpsc::Receiver<WsOutgoing>>,
    ctrl_rx: Option<tokio::sync::mpsc::Receiver<WsOutgoing>>,
}

impl<H: WebSocketHandler> WsConnection<H> {
    pub fn new(handler: H, ctx: RequestContext) -> (Self, WsSink) {
        Self::new_shared(Arc::new(handler), ctx)
    }

    /// Same as [`Self::new`] but takes an already-shared handler, so one handler
    /// instance can serve any number of connections.
    pub fn new_shared(handler: Arc<H>, ctx: RequestContext) -> (Self, WsSink) {
        let (data_tx, data_rx) = tokio::sync::mpsc::channel(OUTGOING_QUEUE_CAPACITY);
        let (ctrl_tx, ctrl_rx) = tokio::sync::mpsc::channel(CONTROL_QUEUE_CAPACITY);
        let sink = WsSink::new(data_tx, ctrl_tx);
        let conn = Self {
            handler,
            ctx: Arc::new(ctx),
            sink: sink.clone(),
            data_rx: Some(data_rx),
            ctrl_rx: Some(ctrl_rx),
        };
        (conn, sink)
    }

    /// Drives the connection to completion.
    pub async fn run(mut self, ws_stream: WebSocketStream<TcpStream>) {
        let data_rx = self.data_rx.take().unwrap();
        let ctrl_rx = self.ctrl_rx.take().unwrap();

        self.handler.on_connect(&self.ctx, &self.sink).await;

        let (ws_tx, mut ws_rx) = ws_stream.split();
        let mut writer = tokio::spawn(writer_loop(ws_tx, data_rx, ctrl_rx));

        // The handler runs on its own task so a slow `on_message` cannot stall
        // the socket read. The queue is bounded: once it fills, `send().await`
        // stops draining the socket and the peer feels TCP backpressure instead
        // of the server buffering without limit.
        let (incoming_tx, mut incoming_rx) = tokio::sync::mpsc::channel(INCOMING_QUEUE_CAPACITY);
        let mut worker = {
            let handler = self.handler.clone();
            let ctx = self.ctx.clone();
            let sink = self.sink.clone();
            tokio::spawn(async move {
                while let Some(incoming) = incoming_rx.recv().await {
                    dispatch(&handler, &ctx, &sink, incoming).await;
                }
            })
        };

        // Reading and dispatching overlap on purpose: a handler only ever sees a
        // frame after it has been read off the socket, so parking frames in a
        // queue and only processing them once the peer went away leaves the
        // client waiting on a reply that never arrives.
        while let Some(frame) = ws_rx.next().await {
            let (incoming, terminal) = match frame {
                Ok(Message::Text(t)) => (WsIncoming::Message(t.to_string()), false),
                Ok(Message::Binary(b)) => (
                    WsIncoming::Message(String::from_utf8_lossy(&b).into_owned()),
                    false,
                ),
                Ok(Message::Ping(payload)) => {
                    let _ = self.sink.send_raw(WsOutgoing::InternalPong(payload));
                    continue;
                }
                Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => continue,
                Ok(Message::Close(_)) => (WsIncoming::Close, true),
                Err(e) => (WsIncoming::Error(e.to_string()), true),
            };

            if incoming_tx.send(incoming).await.is_err() {
                break;
            }
            if terminal {
                break;
            }
        }

        // Let the handler finish what it already accepted before reporting the
        // connection closed, so a sink is never unregistered mid-message.
        drop(incoming_tx);
        if tokio::time::timeout(SHUTDOWN_FLUSH_TIMEOUT, &mut worker)
            .await
            .is_err()
        {
            worker.abort();
        }

        // `on_close` may still want to say something, so it runs before the
        // close frame is queued.
        self.handler.on_close(&self.ctx, &self.sink).await;

        // Best effort, and bounded: the writer may already be wedged on a peer
        // that stopped reading, in which case its control queue never drains
        // and an unbounded `send` would keep this connection — and its
        // `on_close` bookkeeping — alive forever.
        let _ = tokio::time::timeout(
            SHUTDOWN_FLUSH_TIMEOUT,
            self.sink.ctrl.send(WsOutgoing::Close(None)),
        )
        .await;

        // `WsSink` is cloneable, so a handler may still hold copies in a
        // long-lived registry and keep the outgoing channels open. Wait for the
        // close frame to flush, then reclaim the writer rather than leaving it
        // parked on a channel nobody drains.
        if tokio::time::timeout(SHUTDOWN_FLUSH_TIMEOUT, &mut writer)
            .await
            .is_err()
        {
            writer.abort();
        }
    }
}

/// Serializes frames onto the socket, preferring control frames so a close or
/// ping is never stuck behind a backlog of application data.
async fn writer_loop(
    mut ws_tx: SplitSink<WebSocketStream<TcpStream>, Message>,
    mut data_rx: tokio::sync::mpsc::Receiver<WsOutgoing>,
    mut ctrl_rx: tokio::sync::mpsc::Receiver<WsOutgoing>,
) {
    loop {
        let outgoing = tokio::select! {
            biased;
            ctrl = ctrl_rx.recv() => match ctrl {
                Some(msg) => msg,
                None => break,
            },
            data = data_rx.recv() => match data {
                Some(msg) => msg,
                None => break,
            },
        };

        let terminal = outgoing.is_terminal();
        if write_frame(&mut ws_tx, outgoing).await.is_err() || terminal {
            break;
        }
    }
}

async fn write_frame(
    ws_tx: &mut SplitSink<WebSocketStream<TcpStream>, Message>,
    outgoing: WsOutgoing,
) -> Result<(), ()> {
    let frame = match outgoing {
        WsOutgoing::Text(t) => Message::Text(t.into()),
        WsOutgoing::Binary(b) => Message::Binary(b.into()),
        WsOutgoing::Ping(d) => Message::Ping(d.into()),
        WsOutgoing::Pong(d) => Message::Pong(d.into()),
        WsOutgoing::InternalPong(d) => Message::Pong(d.into()),
        WsOutgoing::Close(close) => Message::Close(close.and_then(close_frame)),
    };
    ws_tx.send(frame).await.map_err(|_| ())
}

fn close_frame(close: WsClose) -> Option<CloseFrame<'static>> {
    let code = CloseCode::from(close.code);
    if !is_sendable_close_code(close.code) {
        return None;
    }
    Some(CloseFrame {
        code,
        reason: Cow::Owned(close.reason),
    })
}

/// Whether `code` may legally appear in a close frame we send.
///
/// RFC 6455 §7.4: 1004, 1005, 1006 and 1015 are defined by the protocol
/// machinery and must never be transmitted, and 1000-2999 outside the assigned
/// ranges is unassigned. 1012-1014 were registered by IANA after the RFC, so
/// they are sendable, as is the 3000-3999 library range and 4000-4999 for
/// private use.
fn is_sendable_close_code(code: u16) -> bool {
    matches!(code, 1000..=1003 | 1007..=1014 | 3000..=4999)
}

async fn dispatch<H: WebSocketHandler>(
    handler: &H,
    ctx: &RequestContext,
    sink: &WsSink,
    incoming: WsIncoming,
) {
    match incoming {
        WsIncoming::Message(msg) => match serde_json::from_str::<H::Message>(&msg) {
            Ok(parsed) => {
                handler.on_message(parsed, ctx, sink.clone()).await;
            }
            Err(e) => {
                handler.on_error(WsError::Serialize(e), ctx, sink).await;
            }
        },
        WsIncoming::Close => {}
        WsIncoming::Error(e) => {
            handler.on_error(WsError::Protocol(e), ctx, sink).await;
        }
    }
}

pub enum WsIncoming {
    Message(String),
    Close,
    Error(String),
}

// ---------------------------------------------------------------------------
// Path matching
// ---------------------------------------------------------------------------

/// Whether `request_path` is served by `registered_path`, which may contain
/// `:param` segments. Requires an equal segment count, so `/ws/room` never
/// matches `/ws/room/:room`.
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

/// Pulls `:param` values out of a matched pair. Values are handed back
/// untrusted and undecoded, matching the HTTP router's treatment of path
/// params.
pub fn extract_ws_params(
    path: &str,
    registered_path: &str,
) -> HashMap<String, UntrustedString> {
    let mut params = HashMap::new();
    let path_parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let reg_parts: Vec<&str> = registered_path.split('/').filter(|s| !s.is_empty()).collect();

    for (i, reg_part) in reg_parts.iter().enumerate() {
        if let Some(param_name) = reg_part.strip_prefix(':') {
            if let Some(value) = path_parts.get(i) {
                params.insert(param_name.to_string(), UntrustedString::new(value.to_string()));
            }
        }
    }
    params
}

impl RequestContext {
    /// A path parameter captured by the WebSocket route, e.g. the `room` in
    /// `/ws/room/:room`.
    pub fn ws_param(&self, key: &str) -> Option<&UntrustedString> {
        self.params.get(key)
    }

    pub fn ws_params(&self) -> &HashMap<String, UntrustedString> {
        &self.params
    }
}
