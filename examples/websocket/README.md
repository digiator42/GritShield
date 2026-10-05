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
}

register_ws!("/ws/path", |stream, ctx| {
    let (conn, _) = WsConnection::new(MyHandler, ctx);
    conn.run(stream)
});
```
