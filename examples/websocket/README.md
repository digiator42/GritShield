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

    fn on_message(&self, msg: MyMessage, ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
        Box::pin(async move {
            let _ = ws.send(&MyMessage { ... });
        })
    }
}

register_ws!("/ws/path", |stream, ctx| {
    let (conn, _) = WsConnection::new(MyHandler, ctx);
    conn.run(stream)
});
```