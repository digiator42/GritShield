GritShield provides built-in WebSocket support using a simple macro for route registration.

## Overview

WebSockets are registered using the `register_ws!` macro. This macro automatically registers the handler at application startup.

### Register WebSocket Macro

```rust
#[macro_export]
macro_rules! register_ws {
    ($path:expr, $handler:expr) => {
        #[ctor::ctor]
        fn init_ws_route() {
            let wrapped: $crate::routing::websocket::WsHandlerFn = |stream, ctx| {
                Box::pin($handler(stream, ctx))
            };
            $crate::routing::websocket::register_ws_route($path, wrapped);
        }
    };
}
```

## Simple Usage Demo

### 1. Create WebSocket Handler

```rust
use futures_util::sink::SinkExt;
use gritshield::{futures::StreamExt, routing::engine::RequestContext};
use tokio::net::TcpStream;
use tokio_tungstenite::WebSocketStream;

async fn echo_handler(
    mut stream:  WebSocketStream<TcpStream>,
    ctx: RequestContext
) {
    println!("New WebSocket connection from: {}", ctx.peer_addr);

    while let Some(msg) = stream.next().await {
        match msg {
            Ok(message) => {
                if message.is_text() || message.is_binary() {
                    // Echo message back
                    let _ = stream.send(message).await;
                }
            }
            Err(e) => {
                error!("WebSocket error: {}", e);
                break;
            }
        }
    }
}

register_ws!("/ws/echo", echo_handler);
```

## Client Side Test (JavaScript)

```javascript
const ws = new WebSocket("ws://localhost:8080/ws/echo");

ws.onopen = () => {
  console.log("Connected to GritShield WebSocket");
  ws.send("Hello Server!");
};

ws.onmessage = (event) => {
  console.log("Received:", event.data);
};

ws.onclose = () => console.log("Connection closed");
```
