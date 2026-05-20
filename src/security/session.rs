use dashmap::DashMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use uuid::{Timestamp, Uuid};

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
    pub sessions: Arc<Mutex<DashMap<String, Arc<Mutex<Session>>>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(DashMap::new())),
        }
    }

    pub fn get_or_create(&self, id: Option<String>) -> (Arc<Mutex<Session>>, bool) {
        let sessions = self.sessions.lock().unwrap();

        if let Some(sid) = id {
            if let Some(session) = sessions.get(&sid) {
                // Update last_accessed so the reaper doesn't kill an active user
                session.lock().unwrap().last_accessed = Instant::now();
                return (Arc::clone(&session), false);
            }
        }

        let new_id = Uuid::new_v4().to_string();
        let new_session = Arc::new(Mutex::new(Session {
            id: new_id.clone(),
            data: HashMap::new(),
            user_id: None,
            last_accessed: Instant::now(),
        }));

        sessions.insert(new_id, Arc::clone(&new_session));
        (new_session, true)
    }

    pub fn spawn_reaper(store: Arc<SessionStore>, timeout: Duration) {
        thread::spawn(move || {
            loop {
                // Sleep first to avoid high CPU usage
                thread::sleep(Duration::from_secs(60));

                let mut sessions = store.sessions.lock().unwrap();
                let now = Instant::now();

                // Retain only the sessions that haven't expired
                sessions.retain(|id, session_ptr| {
                    let last = session_ptr.lock().unwrap().last_accessed;
                    if now.duration_since(last) > timeout {
                        println!("[REAPER] Cleaning up expired session: {}", id);
                        false // Remove from HashMap
                    } else {
                        true // Keep it
                    }
                });
            }
        });
    }
}
