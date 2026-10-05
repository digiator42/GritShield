//! Wire-level tests for `WsConnection::run`.
//!
//! The regression these guard: `run()` used to drain the socket into a queue and
//! only invoke the handler once the peer had disconnected, so a client sending a
//! frame on a live connection never saw the echo or the broadcast.

use gritshield::deps::futures_util::{SinkExt, StreamExt};
use gritshield::deps::serde::{Deserialize, Serialize};
use gritshield::deps::tokio_tungstenite::tungstenite::Message;
use gritshield::deps::tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use gritshield::routing::engine::RequestContext;
use gritshield::routing::websocket::{
    BoxedWsFuture, WsConnection, WsError, WsOutgoing, WsSink, WebSocketHandler,
};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

type ClientStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Debug, Serialize, Deserialize)]
struct Chat {
    user: String,
    text: String,
}

/// Events the handler reports back to the test.
#[derive(Debug)]
enum Event {
    Connected(usize),
    Closed(usize),
    Error(String),
}

struct EchoHandler {
    events: UnboundedSender<Event>,
}

impl WebSocketHandler for EchoHandler {
    type Message = Chat;

    fn on_connect(&self, _ctx: &RequestContext, ws: &WsSink) -> BoxedWsFuture {
        let _ = self.events.send(Event::Connected(ws.id()));
        Box::pin(async {})
    }

    fn on_message(&self, msg: Chat, _ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
        let reply = Chat {
            user: "server".into(),
            text: format!("echo: {}", msg.text),
        };
        let payload = serde_json::to_string(&reply).unwrap();
        Box::pin(async move {
            let _ = ws.send_raw(WsOutgoing::Text(payload));
        })
    }

    fn on_close(&self, _ctx: &RequestContext, ws: &WsSink) -> BoxedWsFuture {
        let _ = self.events.send(Event::Closed(ws.id()));
        Box::pin(async {})
    }

    fn on_error(&self, err: WsError, _ctx: &RequestContext) -> BoxedWsFuture {
        let _ = self.events.send(Event::Error(err.to_string()));
        Box::pin(async {})
    }
}

/// Same shape as `EchoHandler` but with no event feed — needed where the handler
/// has to be a plain `fn` pointer for the route registry.
struct PlainEcho;

impl WebSocketHandler for PlainEcho {
    type Message = Chat;

    fn on_message(&self, msg: Chat, _ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
        let reply = Chat {
            user: "server".into(),
            text: format!("echo: {}", msg.text),
        };
        Box::pin(async move {
            let _ = ws.send(&reply);
        })
    }
}

/// Spawns a server that runs a single WS connection and returns its address plus
/// the handler event feed.
async fn spawn_server() -> (SocketAddr, UnboundedReceiver<Event>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (events_tx, events_rx) = unbounded_channel();

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let ws_stream = gritshield::deps::tokio_tungstenite::accept_async(stream)
            .await
            .unwrap();
        let (conn, _sink) =
            WsConnection::new(EchoHandler { events: events_tx }, RequestContext::new());
        conn.run(ws_stream).await;
    });

    (addr, events_rx)
}

/// Reads the next text frame, failing the test if the server stays silent.
async fn next_text(rx: &mut ClientStream) -> String {
    let frame = tokio::time::timeout(Duration::from_secs(3), rx.next())
        .await
        .expect("server sent nothing while the connection was still open")
        .expect("socket closed unexpectedly")
        .expect("websocket error");

    match frame {
        Message::Text(t) => t.to_string(),
        other => panic!("expected a text frame, got {:?}", other),
    }
}

#[tokio::test]
async fn echo_is_delivered_before_the_client_disconnects() {
    let (addr, _events) = spawn_server().await;

    let (mut ws, _resp) = gritshield::deps::tokio_tungstenite::connect_async(format!("ws://{}", addr))
        .await
        .unwrap();

    // Three frames back-to-back on a single live connection. Each one must come
    // back immediately; none of them may be held until the socket closes.
    for i in 0..3 {
        ws.send(Message::Text(format!("{{\"user\":\"u\",\"text\":\"m{}\"}}", i).into()))
            .await
            .unwrap();
        let echoed: Chat = serde_json::from_str(&next_text(&mut ws).await).unwrap();
        assert_eq!(echoed.user, "server");
        assert_eq!(echoed.text, format!("echo: m{}", i));
    }

    ws.close(None).await.unwrap();
}

#[tokio::test]
async fn connect_and_close_share_one_sink_identity() {
    let (addr, mut events) = spawn_server().await;

    let (mut ws, _resp) = gritshield::deps::tokio_tungstenite::connect_async(format!("ws://{}", addr))
        .await
        .unwrap();

    let connected_id = match tokio::time::timeout(Duration::from_secs(3), events.recv())
        .await
        .expect("on_connect never fired")
    {
        Some(Event::Connected(id)) => id,
        other => panic!("expected Event::Connected, got {:?}", other),
    };

    ws.close(None).await.unwrap();

    let closed_id = match tokio::time::timeout(Duration::from_secs(3), events.recv())
        .await
        .expect("on_close never fired")
    {
        Some(Event::Closed(id)) => id,
        other => panic!("expected Event::Closed, got {:?}", other),
    };

    // `WsSink::id()` is the key a connection registry needs: registering on
    // connect and dropping on close must land on the same entry.
    assert_eq!(connected_id, closed_id);
}

#[tokio::test]
async fn malformed_payload_is_reported_to_on_error() {
    let (addr, mut events) = spawn_server().await;

    let (mut ws, _resp) = gritshield::deps::tokio_tungstenite::connect_async(format!("ws://{}", addr))
        .await
        .unwrap();


    ws.send(Message::Text("not json at all".into())).await.unwrap();

    let mut error = None;
    while error.is_none() {
        match tokio::time::timeout(Duration::from_secs(3), events.recv())
            .await
            .expect("on_error never fired")
        {
            Some(Event::Error(msg)) => error = Some(msg),
            Some(Event::Connected(_)) | Some(Event::Closed(_)) => {}
            None => break,
        }
    }

    let msg = error.expect("no Event::Error was reported");
    assert!(msg.contains("Serialization failed"), "got {}", msg);

    // A bad frame must not take the connection down with it.
    ws.send(Message::Text("{\"user\":\"u\",\"text\":\"still alive\"}".into()))
        .await
        .unwrap();
    let echoed: Chat = serde_json::from_str(&next_text(&mut ws).await).unwrap();
    assert_eq!(echoed.text, "echo: still alive");
}

/// Same check, but driven through `handle_connection` so the HTTP upgrade
/// handshake and the WS route registry are covered too.
#[tokio::test]
async fn echo_survives_the_full_http_upgrade_path() {
    const PATH: &str = "/ws/test-upgrade";

    gritshield::routing::websocket::register_ws_route(PATH, |stream, ctx| {
        let (conn, _sink) = WsConnection::new(PlainEcho, ctx);
        Box::pin(conn.run(stream))
    });

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Arc::new(gritshield::routing::engine::Router::new());

    tokio::spawn(async move {
        while let Ok((stream, peer)) = listener.accept().await {
            let router = router.clone();
            tokio::spawn(async move {
                gritshield::http::handle_connection(stream, peer, router).await;
            });
        }
    });

    let url = format!("ws://{}{}", addr, PATH);
    let (mut ws, resp) = gritshield::deps::tokio_tungstenite::connect_async(&url)
        .await
        .expect("handshake with the registered WS route failed");
    assert_eq!(resp.status().as_u16(), 101);

    ws.send(Message::Text("{\"user\":\"u\",\"text\":\"hello\"}".into()))
        .await
        .unwrap();

    let echoed: Chat = serde_json::from_str(&next_text(&mut ws).await).unwrap();
    assert_eq!(echoed.text, "echo: hello");

    ws.close(None).await.unwrap();
}

/// Sink ids are unique per connection, so a shared registry keyed by them can
/// never collapse two live clients into one entry.
#[tokio::test]
async fn sink_ids_are_unique_per_connection() {
    let (addr_a, _events_a) = spawn_server().await;
    let (addr_b, _events_b) = spawn_server().await;

    let (mut a, _) = gritshield::deps::tokio_tungstenite::connect_async(format!("ws://{}", addr_a))
        .await
        .unwrap();
    let (mut b, _) = gritshield::deps::tokio_tungstenite::connect_async(format!("ws://{}", addr_b))
        .await
        .unwrap();

    a.send(Message::Text("{\"user\":\"a\",\"text\":\"1\"}".into())).await.unwrap();
    b.send(Message::Text("{\"user\":\"b\",\"text\":\"1\"}".into())).await.unwrap();

    let first: Chat = serde_json::from_str(&next_text(&mut a).await).unwrap();
    let second: Chat = serde_json::from_str(&next_text(&mut b).await).unwrap();
    assert_eq!(first.text, "echo: 1");
    assert_eq!(second.text, "echo: 1");

    a.close(None).await.unwrap();
    b.close(None).await.unwrap();
}
