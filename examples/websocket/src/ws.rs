use gritshield::prelude::*;
use gritshield::routing::websocket::{WebSocketHandler, WsSink, WsError, BoxedWsFuture};
use gritshield::routing::engine::RequestContext;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub user: String,
    pub text: String,
    pub room: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BroadcastMessage {
    pub from: String,
    pub content: String,
    pub timestamp: String,
}

static BROADCAST: std::sync::OnceLock<broadcast::Sender<BroadcastMessage>> = std::sync::OnceLock::new();

fn get_broadcast() -> broadcast::Sender<BroadcastMessage> {
    BROADCAST
        .get_or_init(|| broadcast::channel::<BroadcastMessage>(32).0)
        .clone()
}

struct EchoHandler;

impl WebSocketHandler for EchoHandler {
    type Message = ChatMessage;

    fn on_connect(&self, ctx: &RequestContext) -> BoxedWsFuture {
        let peer = ctx.peer_addr;
        info!("[WS] New echo connection from: {}", peer);
        Box::pin(async move {})
    }

    fn on_message(&self, msg: ChatMessage, ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
        let user = msg.user.clone();
        let text = msg.text.clone();
        let room = msg.room.clone();
        let peer = ctx.peer_addr;

        let broadcast_tx = get_broadcast();

        Box::pin(async move {
            info!("[WS] {} from {}: {}", text, user, peer);

            let echo = ChatMessage {
                user: "server".into(),
                text: format!("echo: {}", text),
                room: room.clone(),
            };
            let _ = ws.send(&echo);

            let broadcast_msg = BroadcastMessage {
                from: user,
                content: text,
                timestamp: chrono::Utc::now().to_rfc3339(),
            };
            let _ = broadcast_tx.send(broadcast_msg);
        })
    }

    fn on_close(&self, ctx: &RequestContext) -> BoxedWsFuture {
        let peer = ctx.peer_addr;
        info!("[WS] Echo connection closed: {}", peer);
        Box::pin(async move {})
    }

    fn on_error(&self, err: WsError, ctx: &RequestContext) -> BoxedWsFuture {
        let peer = ctx.peer_addr;
        error!("[WS] Echo error for {}: {}", peer, err);
        Box::pin(async move {})
    }
}

struct BroadcastHandler;

impl WebSocketHandler for BroadcastHandler {
    type Message = ChatMessage;

    fn on_connect(&self, ctx: &RequestContext) -> BoxedWsFuture {
        let peer = ctx.peer_addr;
        info!("[WS] Broadcast connection from: {}", peer);
        Box::pin(async move {})
    }

    fn on_message(&self, msg: ChatMessage, _ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
        let broadcast_tx = get_broadcast();

        Box::pin(async move {
            let broadcast_msg = BroadcastMessage {
                from: msg.user,
                content: msg.text,
                timestamp: chrono::Utc::now().to_rfc3339(),
            };
            let _ = broadcast_tx.send(broadcast_msg);

            let ack = ChatMessage {
                user: "server".into(),
                text: "broadcast sent".into(),
                room: None,
            };
            let _ = ws.send(&ack);
        })
    }

    fn on_close(&self, ctx: &RequestContext) -> BoxedWsFuture {
        let peer = ctx.peer_addr;
        info!("[WS] Broadcast connection closed: {}", peer);
        Box::pin(async move {})
    }
}

struct RoomHandler;

impl WebSocketHandler for RoomHandler {
    type Message = ChatMessage;

    fn on_connect(&self, ctx: &RequestContext) -> BoxedWsFuture {
        let room = ctx.ws_param("room").map(|s| s.to_string()).unwrap_or_else(|| "default".into());
        let peer = ctx.peer_addr;
        info!("[WS] Room '{}' connection from: {}", room, peer);
        Box::pin(async move {})
    }

    fn on_message(&self, msg: ChatMessage, ctx: &RequestContext, ws: WsSink) -> BoxedWsFuture {
        let room = ctx.ws_param("room").map(|s| s.to_string()).unwrap_or_else(|| "default".into());

        Box::pin(async move {
            info!("[WS] Room '{}' message from {}: {}", room, msg.user, msg.text);

            let response = ChatMessage {
                user: "server".into(),
                text: format!("[room: {}] {}", room, msg.text),
                room: Some(room.clone()),
            };
            let _ = ws.send(&response);
        })
    }

    fn on_close(&self, ctx: &RequestContext) -> BoxedWsFuture {
        let room = ctx.ws_param("room").map(|s| s.to_string()).unwrap_or_else(|| "default".into());
        let peer = ctx.peer_addr;
        info!("[WS] Room '{}' connection closed: {}", room, peer);
        Box::pin(async move {})
    }
}

mod ws_echo {
    use super::*;
    register_ws!("/ws/echo", |stream, ctx| {
        let handler = EchoHandler;
        let (conn, _sink) = gritshield::routing::websocket::WsConnection::new(handler, ctx);
        conn.run(stream)
    });
}

mod ws_broadcast {
    use super::*;
    register_ws!("/ws/broadcast", |stream, ctx| {
        let handler = BroadcastHandler;
        let (conn, _sink) = gritshield::routing::websocket::WsConnection::new(handler, ctx);
        conn.run(stream)
    });
}

mod ws_room {
    use super::*;
    register_ws!("/ws/room/:room", |stream, ctx| {
        let handler = RoomHandler;
        let (conn, _sink) = gritshield::routing::websocket::WsConnection::new(handler, ctx);
        conn.run(stream)
    });
}