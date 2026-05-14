use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub struct RateLimiter {
    // Maps IP -> (Request Count, Window Start Time)
    pub visitors: Arc<Mutex<HashMap<String, (u32, Instant)>>>,
    pub max_requests: u32,
    pub window: Duration,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            visitors: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window: Duration::from_secs(window_secs),
        }
    }

    pub fn is_allowed(&self, ip: String) -> bool {
        let mut visitors = self.visitors.lock().unwrap();
        let now = Instant::now();

        let (count, start_time) = visitors.entry(ip).or_insert((0, now));

        if now.duration_since(*start_time) > self.window {
            // Window expired, reset counter
            *count = 1;
            *start_time = now;
            true
        } else {
            if *count < self.max_requests {
                *count += 1;
                true
            } else {
                false
            }
        }
    }
}
