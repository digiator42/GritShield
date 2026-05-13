use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

pub struct Session {
    pub id: String,
    pub data: HashMap<String, String>,
}

pub struct SessionStore {
    pub sessions: Arc<Mutex<HashMap<String, Arc<Mutex<Session>>>>>,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get_or_create(&self, id: Option<String>) -> (Arc<Mutex<Session>>, bool) {
        let mut sessions = self.sessions.lock().unwrap();

        if let Some(sid) = id {
            if let Some(session) = sessions.get(&sid) {
                return (Arc::clone(session), false);
            }
        }

        let new_id = Uuid::new_v4().to_string();
        let new_session = Arc::new(Mutex::new(Session {
            id: new_id.clone(),
            data: HashMap::new(),
        }));

        sessions.insert(new_id, Arc::clone(&new_session));
        (new_session, true)
    }
}
