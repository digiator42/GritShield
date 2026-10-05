GritShield provides built-in WebSocket support using a trait-based handler system with lifecycle hooks, automatic JSON message serialization, and path parameter extraction.

## Overview

WebSockets are registered using the `register_ws!` macro with a handler function. For better DX, implement the `WebSocketHandler` trait which provides structured lifecycle hooks (`on_connect`, `on_message`, `on_close`, `on_error`) and automatic JSON message handling.

## Quick Start

### 1. Define Your Message Type

```rust
use gritshield::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub user: String,
    pub text: String,
    pub room: Option<String>,
}
```

### 2. Implement WebSocketHandler

```rust
use gritshield::prelude::*;
use gritshield::routing::websocket::{WebSocketHandler, WsSink, WsError, BoxedWsFuture};
use gritshield::routing::engine::RequestContext;

struct EchoHandler;

impl WebSocketHandler for EchoHandler {
    type Message = ChatMessage;

    fn on_connect(&self, ctx: &RequestContext) -> BoxedWsFuture {
        info!("[WS] New connection from: {}", ctx.peer_addr);
        Box::pin(async move {})
    }

    fn on_message(&self, msg: ChatMessage, ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
        let peer = ctx.peer_addr;
        Box::pin(async move {
            info!("[WS] Message from {}: {}", peer, msg.text);

            // Echo back
            let echo = ChatMessage {
                user: "server".into(),
                text: format!("echo: {}", msg.text),
                room: msg.room,
            };
            let _ = ws.send(&echo);
        })
    }

    fn on_close(&self, ctx: &RequestContext) -> BoxedWsFuture {
        info!("[WS] Connection closed: {}", ctx.peer_addr);
        Box::pin(async move {})
    }

    fn on_error(&self, err: WsError, ctx: &RequestContext) -> BoxedWsFuture {
        error!("[WS] Error for {}: {}", ctx.peer_addr, err);
        Box::pin(async move {})
    }
}
```

### 3. Register the WebSocket Route

```rust
register_ws!("/ws/echo", |stream, ctx| {
    let handler = EchoHandler;
    let (conn, _sink) = gritshield::routing::websocket::WsConnection::new(handler, ctx);
    conn.run(stream)
});
```

## Path Parameters

Extract dynamic path parameters using `ctx.ws_param()`:

```rust
register_ws!("/ws/room/:room", |stream, ctx| {
    let handler = RoomHandler;
    let (conn, _sink) = gritshield::routing::websocket::WsConnection::new(handler, ctx);
    conn.run(stream)
});

struct RoomHandler;
impl WebSocketHandler for RoomHandler {
    type Message = ChatMessage;

    fn on_connect(&self, ctx: &RequestContext) -> BoxedWsFuture {
        // Extract the :room parameter
        let room = ctx.ws_param("room").map(|s| s.to_string()).unwrap_or_else(|| "default".into());
        info!("[WS] Joined room: {}", room);
        Box::pin(async move {})
    }
    // ...
}
```

## WebSocketHandler Trait

```rust
pub trait WebSocketHandler: Send + Sync + 'static {
    type Message: Serialize + DeserializeOwned + Send + 'static;

    // Called when client connects
    fn on_connect(&self, _ctx: &RequestContext) -> BoxedWsFuture {
        Box::pin(async {})
    }

    // Called for each incoming message (auto-deserialized from JSON)
    fn on_message(&self, msg: Self::Message, ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture;

    // Called when client disconnects
    fn on_close(&self, _ctx: &RequestContext) -> BoxedWsFuture {
        Box::pin(async {})
    }

    // Called on protocol or serialization errors
    fn on_error(&self, _err: WsError, _ctx: &RequestContext) -> BoxedWsFuture {
        Box::pin(async {})
    }
}
```

## WsSink - Sending Messages

The `WsSink` provides a simple API for sending messages back to the client:

```rust
impl WsSink {
    // Send any serializable type as JSON
    pub fn send<T: Serialize>(&self, msg: &T) -> Result<(), WsError>;

    // Send raw WebSocket frames
    pub fn send_raw(&self, msg: WsOutgoing) -> Result<(), WsError>;

    // Close the connection
    pub fn close(&self) -> Result<(), WsError>;

    // Send ping
    pub fn ping(&self, data: Vec<u8>) -> Result<(), WsError>;
}

pub enum WsOutgoing {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close,
}
```

## Complete Example

See `examples/websocket` for a full working example with:
- Echo server
- Broadcast to all connected clients
- Room-based chat with path parameters

Run it with:
```bash
cd examples/websocket
cargo run
```

Then open `http://localhost:8087` in your browser to test.

## Client-Side Test (JavaScript)

```javascript
const ws = new WebSocket("ws://localhost:8087/ws/echo");

ws.onopen = () => {
  console.log("Connected");
  ws.send(JSON.stringify({ user: "alice", text: "Hello!", room: null }));
};

ws.onmessage = (event) => {
  console.log("Received:", event.data);
};

ws.onclose = () => console.log("Disconnected");
```