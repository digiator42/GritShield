//! Wire-level tests for the WebSocket stack.
//!
//! Two layers are covered here:
//!
//! * `WsConnection::run` on its own, driven straight over a socket, because the
//!   regression these guard is in the read/dispatch loop: `run()` used to drain
//!   the socket into a queue and only invoke the handler once the peer had
//!   disconnected, so a client sending a frame on a live connection never saw
//!   the echo or the broadcast.
//! * The whole upgrade path through `handle_connection`, because routing,
//!   handshake validation and subprotocol negotiation all live there and a
//!   `run()`-only test cannot see them.

use gritshield::deps::futures_util::{SinkExt, StreamExt};
use gritshield::deps::serde::{Deserialize, Serialize};
use gritshield::deps::tokio_tungstenite::tungstenite::Message;
use gritshield::deps::tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use gritshield::routing::engine::RequestContext;
use gritshield::routing::websocket::{
    BoxedWsFuture, WsConnection, WsError, WsOutgoing, WsSink, WebSocketHandler,
    register_ws_route, register_ws_route_with_subprotocols,
};
use gritshield::testing::{WsTestServer, TEST_WS_VERSION};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
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

    fn on_error(&self, err: WsError, _ctx: &RequestContext, _ws: &WsSink) -> BoxedWsFuture {
        let _ = self.events.send(Event::Error(err.to_string()));
        Box::pin(async {})
    }
}

/// Same shape as `EchoHandler` but with no event feed — needed where the handler
/// has to be cheaply constructible for the route registry.
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
async fn malformed_payload_is_reported_to_on_error_and_the_survives() {
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

// ---------------------------------------------------------------------------
// Upgrade path
// ---------------------------------------------------------------------------

fn chat(text: &str) -> String {
    format!("{{\"user\":\"u\",\"text\":\"{}\"}}", text)
}

#[tokio::test]
async fn echo_survives_the_full_http_upgrade_path() {
    const PATH: &str = "/ws/test-upgrade";

    register_ws_route(PATH, |stream, ctx| {
        let (conn, _sink) = WsConnection::new(PlainEcho, ctx);
        Box::pin(conn.run(stream))
    });

    let server = WsTestServer::start().await;
    let mut client = server.connect(PATH).await;

    client.send_text(chat("hello")).await.unwrap();
    let echoed: Chat = serde_json::from_str(&client.expect_text().await).unwrap();
    assert_eq!(echoed.text, "echo: hello");

    client.close().await.unwrap();
}

#[tokio::test]
async fn a_browser_navigating_to_a_ws_route_gets_426_not_404() {
    const PATH: &str = "/ws/test-plain-get";

    register_ws_route(PATH, |_stream, _ctx| Box::pin(std::future::pending::<()>()));

    let server = WsTestServer::start().await;
    let response = server.get(PATH).await;

    assert_eq!(response.status, 426);
    assert!(
        response.body.contains("Upgrade Required"),
        "body was {:?}",
        response.body
    );
}

#[tokio::test]
async fn an_unregistered_ws_style_path_still_404s() {
    let server = WsTestServer::start().await;
    let response = server.get("/ws/nothing-here").await;

    assert_ne!(response.status, 426);
}

#[tokio::test]
async fn an_unsupported_websocket_version_is_refused_with_426() {
    const PATH: &str = "/ws/test-version";

    register_ws_route(PATH, |_stream, _ctx| Box::pin(std::future::pending::<()>()));

    let server = WsTestServer::start().await;
    let failure = server
        .try_connect_with_version(PATH, "8")
        .await
        .expect_err("version 8 must not be upgraded");

    assert_eq!(failure.status(), Some(426));
    assert_eq!(
        failure.header("sec-websocket-version").as_deref(),
        Some(TEST_WS_VERSION),
        "a 426 for version negotiation must advertise the version we speak"
    );
}

#[tokio::test]
async fn a_handshake_without_a_key_is_a_400() {
    const PATH: &str = "/ws/test-no-key";

    register_ws_route(PATH, |_stream, _ctx| Box::pin(std::future::pending::<()>()));

    let server = WsTestServer::start().await;
    let failure = server
        .try_connect_without_key(PATH)
        .await
        .expect_err("a handshake with no key must not upgrade");

    assert_eq!(failure.status(), Some(400));
}

#[tokio::test]
async fn a_route_negotiates_a_shared_subprotocol_by_server_preference() {
    const PATH: &str = "/ws/test-subprotocol";

    // Server order decides, same as HTTP `Accept`: the route advertises v2
    // first, so a client offering both gets v2 rather than whatever it listed
    // first.
    register_ws_route_with_subprotocols(PATH, &["grit.v2", "grit.v1"], |stream, ctx| {
        let (conn, _sink) = WsConnection::new(PlainEcho, ctx);
        Box::pin(conn.run(stream))
    });

    let server = WsTestServer::start().await;

    let mut client = server
        .connect_with_subprotocols(PATH, &["grit.v1", "grit.v2"])
        .await;
    assert_eq!(client.subprotocol(), Some("grit.v2"));

    client.send_text(chat("sub")).await.unwrap();
    let echoed: Chat = serde_json::from_str(&client.expect_text().await).unwrap();
    assert_eq!(echoed.text, "echo: sub");
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_route_with_no_shared_subprotocol_is_still_upgraded_without_one() {
    const PATH: &str = "/ws/test-subprotocol-miss";

    register_ws_route_with_subprotocols(PATH, &["grit.v1"], |stream, ctx| {
        let (conn, _sink) = WsConnection::new(PlainEcho, ctx);
        Box::pin(conn.run(stream))
    });

    let server = WsTestServer::start().await;
    let mut client = server
        .connect_with_subprotocols(PATH, &["something.else"])
        .await;

    assert_eq!(
        client.subprotocol(),
        None,
        "the server must not echo a protocol the client did not offer"
    );

    client.send_text(chat("nope")).await.unwrap();
    let echoed: Chat = serde_json::from_str(&client.expect_text().await).unwrap();
    assert_eq!(echoed.text, "echo: nope");
    client.close().await.unwrap();
}

#[tokio::test]
async fn a_static_ws_segment_beats_a_param_segment_whatever_the_registration_order() {
    // Registered param route first, then the static one. The old linear scan
    // would have handed `/ws/pick/exact` to the `:what` route.
    register_ws_route("/ws/pick/:what", |_stream, _ctx| {
        Box::pin(async move {
            let _ = &mut ();
        })
    });
    register_ws_route("/ws/pick/exact", |stream, ctx| {
        let (conn, _sink) = WsConnection::new(PlainEcho, ctx);
        Box::pin(conn.run(stream))
    });

    let server = WsTestServer::start().await;
    let mut client = server.connect("/ws/pick/exact").await;
    client.send_text(chat("static")).await.unwrap();

    let echoed: Chat = serde_json::from_str(&client.expect_text().await).unwrap();
    assert_eq!(echoed.text, "echo: static");
    client.close().await.unwrap();
}

#[tokio::test]
async fn ws_path_params_reach_the_handler_context() {
    static SEEN: AtomicUsize = AtomicUsize::new(0);

    struct ParamEcho;

    impl WebSocketHandler for ParamEcho {
        type Message = Chat;

        fn on_connect(&self, ctx: &RequestContext, ws: &WsSink) -> BoxedWsFuture {
            let room = ctx.ws_param("room").map(|v| v.to_string()).unwrap_or_else(|| "<none>".into());
            SEEN.store(room.len(), Ordering::SeqCst);
            let _ = ws;
            Box::pin(async {})
        }

        fn on_message(&self, msg: Chat, ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
            let room = ctx.ws_param("room").map(|v| v.to_string()).unwrap_or_else(|| "<none>".into());
            let reply = Chat {
                user: "server".into(),
                text: format!("{}@{}", msg.text, room),
            };
            Box::pin(async move {
                let _ = ws.send(&reply);
            })
        }
    }

    register_ws_route("/ws/param/:room", |stream, ctx| {
        let (conn, _sink) = WsConnection::new(ParamEcho, ctx);
        Box::pin(conn.run(stream))
    });

    let server = WsTestServer::start().await;
    let mut client = server.connect("/ws/param/general").await;
    client.send_text(chat("hi")).await.unwrap();

    let echoed: Chat = serde_json::from_str(&client.expect_text().await).unwrap();
    assert_eq!(echoed.text, "hi@general");
    assert_eq!(SEEN.load(Ordering::SeqCst), "general".len());
    client.close().await.unwrap();
}

#[tokio::test]
async fn the_active_connection_gauge_is_held_for_the_life_of_the_socket() {
    const PATH: &str = "/ws/test-telemetry";

    register_ws_route(PATH, |stream, ctx| {
        let (conn, _sink) = WsConnection::new(PlainEcho, ctx);
        Box::pin(conn.run(stream))
    });

    let server = WsTestServer::start().await;
    let gauge = &server.router().telemetry.active_connections;

    let client = server.connect(PATH).await;

    // The old code released the gauge the instant the upgrade completed, so a
    // metrics scrape during a live session reported zero connections.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        gauge.load(Ordering::Relaxed) >= 1,
        "active connection gauge fell back to {} while a session was open",
        gauge.load(Ordering::Relaxed)
    );

    drop(client);

    // ...and it must come back down when the socket actually goes away.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while gauge.load(Ordering::Relaxed) > 0 && std::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert_eq!(
        gauge.load(Ordering::Relaxed),
        0,
        "active connection gauge never returned to zero"
    );
}

#[tokio::test]
async fn an_invalid_close_code_is_rejected_before_it_reaches_the_socket() {
    const PATH: &str = "/ws/test-close-code";

    register_ws_route(PATH, |stream, ctx| {
        let (conn, _sink) = WsConnection::new(PlainEcho, ctx);
        Box::pin(conn.run(stream))
    });

    let server = WsTestServer::start().await;
    let mut client = server.connect(PATH).await;
    client.send_text(chat("open")).await.unwrap();
    client.expect_text().await;

    // 1005 is "no status received" and must never be transmitted; 999 is
    // unassigned. Both have to be refused rather than silently rewritten.
    let (conn, sink) = WsConnection::new(PlainEcho, RequestContext::new());
    assert!(matches!(
        sink.close_with(1005, "nope"),
        Err(WsError::InvalidCloseCode(1005))
    ));
    assert!(matches!(
        sink.close_with(999, "nope"),
        Err(WsError::InvalidCloseCode(999))
    ));
    assert!(sink.close_with(1000, "bye").is_ok());
    drop(conn);

    client.close().await.unwrap();
}

#[tokio::test]
async fn a_shared_handler_behind_an_arc_serves_many_connections() {
    const PATH: &str = "/ws/test-arc-handler";

    static HITS: AtomicUsize = AtomicUsize::new(0);

    struct CountingEcho;

    impl WebSocketHandler for CountingEcho {
        type Message = Chat;

        fn on_message(&self, msg: Chat, _ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
            HITS.fetch_add(1, Ordering::SeqCst);
            let reply = Chat {
                user: "server".into(),
                text: msg.text,
            };
            Box::pin(async move {
                let _ = ws.send(&reply);
            })
        }
    }

    let handler = Arc::new(CountingEcho);
    let for_route = handler.clone();

    register_ws_route(PATH, move |stream, ctx| {
        let (conn, _sink) = WsConnection::new_shared(for_route.clone(), ctx);
        Box::pin(conn.run(stream))
    });

    let server = WsTestServer::start().await;
    let mut a = server.connect(PATH).await;
    let mut b = server.connect(PATH).await;

    a.send_text(chat("from-a")).await.unwrap();
    let from_a: Chat = serde_json::from_str(&a.expect_text().await).unwrap();
    assert_eq!(from_a.text, "from-a");

    b.send_text(chat("from-b")).await.unwrap();
    let from_b: Chat = serde_json::from_str(&b.expect_text().await).unwrap();
    assert_eq!(from_b.text, "from-b");

    assert_eq!(HITS.load(Ordering::SeqCst), 2);

    a.close().await.unwrap();
    b.close().await.unwrap();
}

/// Messages must be handled in arrival order, one at a time, per connection.
/// A handler that sleeps must not see message two overtake message one.
#[tokio::test]
async fn messages_are_dispatched_in_order_per_connection() {
    const PATH: &str = "/ws/test-ordering";

    register_ws_route(PATH, |stream, ctx| {
        let (conn, _sink) = WsConnection::new(OrderedEcho, ctx);
        Box::pin(conn.run(stream))
    });

    let server = WsTestServer::start().await;
    let mut client = server.connect(PATH).await;

    for i in 0..8 {
        client.send_text(chat(&format!("m{}", i))).await.unwrap();
    }

    for i in 0..8 {
        let echoed: Chat = serde_json::from_str(&client.expect_text().await).unwrap();
        assert_eq!(echoed.text, format!("m{}", i), "frame {} arrived out of order", i);
    }

    client.close().await.unwrap();
}

struct OrderedEcho;

impl WebSocketHandler for OrderedEcho {
    type Message = Chat;

    fn on_message(&self, msg: Chat, _ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
        // A slow handler: if dispatch ran concurrently the replies would race.
        Box::pin(async move {
            tokio::time::sleep(Duration::from_millis(15)).await;
            let reply = Chat {
                user: "server".into(),
                text: msg.text,
            };
            let _ = ws.send(&reply);
        })
    }
}

// The `ws_handler!` macro has to keep producing a handler whose `on_error` gets
// the connection's sink, and whose optional hooks are all wired up.
gritshield::ws_handler!(
    MacroHandler,
    message = Chat,
    on_connect = |_ctx: &RequestContext, ws: &WsSink| {
        let _ = ws;
        Box::pin(async {})
    },
    on_message = |msg: Chat, _ctx: &RequestContext, ws: WsSink| {
        let reply = Chat {
            user: "macro".into(),
            text: msg.text,
        };
        Box::pin(async move {
            let _ = ws.send(&reply);
        })
    },
    on_close = |_ctx: &RequestContext, ws: &WsSink| {
        let _ = ws;
        Box::pin(async {})
    },
    on_error = |err: WsError, _ctx: &RequestContext, ws: &WsSink| {
        // `ws` must be in scope: the macro gained this parameter so an error
        // handler can close or address the connection that failed.
        let _ = (err, ws);
        Box::pin(async {})
    },
);

#[tokio::test]
async fn the_ws_handler_macro_builds_a_working_handler() {
    const PATH: &str = "/ws/test-macro";

    register_ws_route(PATH, |stream, ctx| {
        let (conn, _sink) = WsConnection::new(MacroHandler, ctx);
        Box::pin(conn.run(stream))
    });

    let server = WsTestServer::start().await;
    let mut client = server.connect(PATH).await;

    client.send_text(chat("through-macro")).await.unwrap();
    let echoed: Chat = serde_json::from_str(&client.expect_text().await).unwrap();
    assert_eq!(echoed.user, "macro");
    assert_eq!(echoed.text, "through-macro");

    client.close().await.unwrap();
}