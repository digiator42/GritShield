use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use uuid::{Uuid};

#[derive(Debug, Clone)]
pub struct Session {
    pub id: String,
    pub data: HashMap<String, String>,
    pub user_id: Option<String>,
    pub last_accessed: Instant,
}

impl Session {
    /// Expose a proxy get method to read directly from the data hashmap
    pub fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }
}

pub struct SessionStore {
    pub sessions: Arc<DashMap<String, Arc<Mutex<Session>>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(DashMap::new()),
        }
    }

    pub fn get_or_create(&self, id: Option<String>) -> (Arc<Mutex<Session>>, bool) {
        if let Some(sid) = id {
            if let Some(session_ref) = self.sessions.get(&sid) {
                // Update last_accessed so the reaper doesn't kill an active user
                let session = session_ref.value().clone();
                session.lock().unwrap().last_accessed = Instant::now();
                return (session, false);
            }
        }

        let new_id = Uuid::new_v4().to_string();
        let new_session = Arc::new(Mutex::new(Session {
            id: new_id.clone(),
            data: HashMap::new(),
            user_id: None,
            last_accessed: Instant::now(),
        }));

        self.sessions.insert(new_id, Arc::clone(&new_session));
        (new_session, true)
    }

    pub fn spawn_reaper(store: Arc<SessionStore>, timeout: Duration) {
        thread::spawn(move || {
            loop {
                // Sleep first to avoid high CPU usage
                thread::sleep(Duration::from_secs(60));

                let now = Instant::now();

                // DashMap's own retain() takes only a short-lived shard lock per
                // bucket as it walks the map — it no longer holds one global lock
                // for the whole scan, so live requests aren't blocked behind it.
                store.sessions.retain(|id, session_ptr| {
                    let last = session_ptr.lock().unwrap().last_accessed;
                    if now.duration_since(last) > timeout {
                        println!("[REAPER] Cleaning up expired session: {}", id);
                        false // Remove from map
                    } else {
                        true // Keep it
                    }
                });
            }
        });
    }
}