# WebSocket Example

Run:
```bash
cargo run
```

Open http://localhost:8087 in browser.

## Endpoints
- `/ws/echo` - Echo with broadcast
- `/ws/broadcast` - Pub/sub to all clients
- `/ws/room/:room` - Room-based chat (try `/ws/room/general`)

Each endpoint broadcasts only to its own connections, and each room only to its
own members, so tabs do not bleed into one another. `:room` reaches the handler
through `ctx.ws_param("room")`.

Opening a WebSocket path in a browser (a plain `GET`) gets `426 Upgrade
Required` rather than a misleading `404`.

## Handler Pattern
```rust
struct MyHandler;
impl WebSocketHandler for MyHandler {
    type Message = MyMessage;

    // `ws` is the connection's sink: `ws.id()` is a stable per-connection key,
    // handy for a registry of live clients.
    fn on_connect(&self, ctx: &RequestContext, ws: &WsSink) -> BoxedWsFuture {
        registry().insert(ws.id(), ws.clone());
        Box::pin(async {})
    }

    fn on_message(&self, msg: MyMessage, ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
        Box::pin(async move {
            let _ = ws.send(&MyMessage { ... });
        })
    }

    fn on_close(&self, ctx: &RequestContext, ws: &WsSink) -> BoxedWsFuture {
        registry().remove(&ws.id());
        Box::pin(async {})
    }

    // `on_error` gets the sink too, so a failed connection can be closed with
    // a code or addressed in a registry.
    fn on_error(&self, err: WsError, ctx: &RequestContext, ws: &WsSink) -> BoxedWsFuture {
        Box::pin(async move { let _ = ws.close_with(1011, "internal error"); })
    }
}

register_ws!("/ws/path", |stream, ctx| {
    let (conn, _) = WsConnection::new(MyHandler, ctx);
    conn.run(stream)
});
```

Messages are dispatched one at a time per connection, in arrival order, so a
slow handler delays its own queue and nothing else.

## Sending

`WsSink` sends are queued, not awaited, and fail fast instead of blocking the
caller — a client that stopped reading cannot wedge the sender:

```rust
let _ = ws.send(&message);          // serialized JSON
let _ = ws.send_text("hello");      // raw text frame
let _ = ws.send_binary(bytes);      // raw binary frame
let _ = ws.send_raw(WsOutgoing::Ping(payload));
let _ = ws.close_with(1000, "bye"); // refuses RFC-illegal codes (1005, 1006, ...)
let _ = ws.close();                 // bare close frame
```

A full outbound queue returns `WsError::Backpressure`, which is the signal to
drop the message rather than to retry.

## Subprotocols

Register the protocols a route speaks and the handshake negotiates one:

```rust
register_ws_route_with_subprotocols("/ws/path", &["grit.v2", "grit.v1"], |stream, ctx| {
    let (conn, _) = WsConnection::new(MyHandler, ctx);
    Box::pin(conn.run(stream))
});
```

The server's order is its preference, like HTTP `Accept`. If the client offers
nothing in common the connection still upgrades, with no
`Sec-WebSocket-Protocol` header.

Only version `13` is accepted; anything else is refused with `426` and a
`Sec-WebSocket-Version: 13` header so the client knows what to retry with.

## Shared handlers

Wrap a handler in an `Arc` to serve many connections from one instance, and
build the connection with `WsConnection::new_shared`:

```rust
let handler = Arc::new(MyHandler);
register_ws_route("/ws/path", move |stream, ctx| {
    let (conn, _) = WsConnection::new_shared(handler.clone(), ctx);
    Box::pin(conn.run(stream))
});
```

## Testing

`gritshield::testing` starts the real server on an ephemeral port and speaks the
handshake by hand, so a test can inspect refusals as well as sessions:

```rust
use gritshield::testing::WsTestServer;

let server = WsTestServer::start().await;
let mut client = server.connect("/ws/echo").await;
client.send_text("{\"user\":\"u\",\"text\":\"hi\"}").await.unwrap();
assert_eq!(client.expect_text().await, "{\"user\":\"server\", ...}");

// Refusals are values, not panics.
let plain = server.get("/ws/echo").await;
assert_eq!(plain.status, 426);

let failure = server.try_connect_with_version("/ws/echo", "8").await;
assert_eq!(failure.unwrap_err().status(), Some(426));
```

`expect_text_matching` skips frames until one passes a predicate, which keeps
tests from depending on the order of unrelated broadcasts.