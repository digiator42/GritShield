use crate::http::sse::SseStream;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Sessions idle for longer than this are reaped.
const DEFAULT_TTL: Duration = Duration::from_secs(900);

/// Hard ceiling on concurrent sessions, so a client looping
/// `GET /mcp/sse` cannot exhaust file descriptors or memory.
const DEFAULT_MAX_SESSIONS: usize = 512;

/// The negotiated state of one connected MCP client.
pub struct McpSession {
    pub id: String,
    pub created_at: String,
    /// The protocol revision agreed during `initialize`.
    protocol_version: Mutex<Option<String>>,
    /// The client's reported identity, from the `initialize` handshake.
    client_info: Mutex<Option<Value>>,
    /// Whether `initialize` has completed.
    pub initialized: AtomicBool,
    /// Monotonic count of requests served on this session.
    request_count: AtomicU64,
    /// Creation instant, used for TTL reaping.
    born: Instant,
    /// The outbound frame channel for this session.
    stream: SseStream,
}

impl McpSession {
    fn new(id: String) -> Self {
        Self {
            created_at: chrono::Local::now().to_rfc3339(),
            id,
            protocol_version: Mutex::new(None),
            client_info: Mutex::new(None),
            initialized: AtomicBool::new(false),
            request_count: AtomicU64::new(0),
            born: Instant::now(),
            stream: SseStream::new(),
        }
    }

    pub fn set_protocol_version(&self, version: &str) {
        if let Ok(mut slot) = self.protocol_version.lock() {
            *slot = Some(version.to_string());
        }
    }

    pub fn protocol_version(&self) -> Option<String> {
        self.protocol_version.lock().ok().and_then(|v| v.clone())
    }

    pub fn set_client_info(&self, info: Value) {
        if let Ok(mut slot) = self.client_info.lock() {
            *slot = Some(info);
        }
    }

    pub fn client_info(&self) -> Option<Value> {
        self.client_info.lock().ok().and_then(|v| v.clone())
    }

    pub fn record_request(&self) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn request_count(&self) -> u64 {
        self.request_count.load(Ordering::Relaxed)
    }

    pub fn age(&self) -> Duration {
        self.born.elapsed()
    }

    /// Push a JSON-RPC payload onto the session's SSE stream.
    pub fn push(&self, event: &str, payload: &Value) -> Result<usize, String> {
        self.stream.send_json(event, payload)
    }

    pub fn stream(&self) -> SseStream {
        self.stream.clone()
    }

    /// The handshake frame announcing where to POST messages.
    pub fn endpoint_path(&self, prefix: &str) -> String {
        format!("{}/message?session_id={}", prefix.trim_end_matches('/'), self.id)
    }
}

/// The process-wide session table.
pub struct McpSessionStore {
    sessions: DashMap<String, Arc<McpSession>>,
    ttl: Duration,
    max_sessions: usize,
}

impl McpSessionStore {
    pub fn new(ttl: Duration, max_sessions: usize) -> Self {
        Self {
            sessions: DashMap::new(),
            ttl,
            max_sessions,
        }
    }

    /// Mint a new session, evicting the oldest idle one if the table is full.
    pub fn create(&self) -> Arc<McpSession> {
        self.prune();

        while self.sessions.len() >= self.max_sessions {
            let oldest = self
                .sessions
                .iter()
                .max_by_key(|entry| entry.value().age())
                .map(|entry| entry.key().clone());

            match oldest {
                Some(key) => {
                    self.sessions.remove(&key);
                }
                None => break,
            }
        }

        let session = Arc::new(McpSession::new(uuid::Uuid::new_v4().to_string()));
        self.sessions.insert(session.id.clone(), Arc::clone(&session));
        session
    }

    pub fn get(&self, id: &str) -> Option<Arc<McpSession>> {
        let session = self.sessions.get(id)?.clone();
        if session.age() > self.ttl {
            self.sessions.remove(id);
            return None;
        }
        Some(session)
    }

    pub fn remove(&self, id: &str) -> Option<Arc<McpSession>> {
        self.sessions.remove(id).map(|(_, session)| session)
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }

    pub fn ids(&self) -> Vec<String> {
        self.sessions.iter().map(|e| e.key().clone()).collect()
    }

    /// Drop every expired session. Returns how many were reaped.
    pub fn prune(&self) -> usize {
        let ttl = self.ttl;
        let expired: Vec<String> = self
            .sessions
            .iter()
            .filter(|entry| entry.value().age() > ttl)
            .map(|entry| entry.key().clone())
            .collect();

        for id in &expired {
            self.sessions.remove(id);
        }

        expired.len()
    }
}

impl Default for McpSessionStore {
    fn default() -> Self {
        Self::new(DEFAULT_TTL, DEFAULT_MAX_SESSIONS)
    }
}

lazy_static::lazy_static! {
    static ref MCP_SESSIONS: McpSessionStore = McpSessionStore::default();
}

/// The process-wide session table.
pub fn sessions() -> &'static McpSessionStore {
    &MCP_SESSIONS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_round_trip_through_the_store() {
        let store = McpSessionStore::default();
        let session = store.create();

        let found = store.get(&session.id).expect("session should be retrievable");
        assert_eq!(found.id, session.id);
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn unknown_session_ids_return_none() {
        let store = McpSessionStore::default();
        assert!(store.get("does-not-exist").is_none());
    }

    #[test]
    fn removal_is_honoured() {
        let store = McpSessionStore::default();
        let session = store.create();
        assert!(store.remove(&session.id).is_some());
        assert!(store.get(&session.id).is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn expired_sessions_are_reaped() {
        let store = McpSessionStore::new(Duration::from_millis(1), 16);
        let session = store.create();
        std::thread::sleep(Duration::from_millis(5));

        assert!(store.get(&session.id).is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn the_table_is_capped() {
        let store = McpSessionStore::new(Duration::from_secs(3600), 4);
        for _ in 0..10 {
            store.create();
        }
        assert!(store.len() <= 4, "session cap breached: {}", store.len());
    }

    #[test]
    fn endpoint_path_carries_the_session_id() {
        let store = McpSessionStore::default();
        let session = store.create();
        assert_eq!(
            session.endpoint_path("/mcp"),
            format!("/mcp/message?session_id={}", session.id)
        );
    }

    #[tokio::test]
    async fn pushes_reach_a_subscriber() {
        let store = McpSessionStore::default();
        let session = store.create();
        let mut receiver = session.stream().subscribe();

        session
            .push("message", &serde_json::json!({ "ok": true }))
            .unwrap();

        let frame = receiver.recv().await.unwrap();
        assert!(frame.contains("event: message"));
        assert!(frame.contains("\"ok\":true"));
    }
}