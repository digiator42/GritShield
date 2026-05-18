use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct RateBucket {
    request_count: usize,
    window_expires_at: Instant,
}

#[derive(Clone)]
pub struct RateLimiter {
    // Shared thread-safe dictionary: Mapping Client IP ->  Their Rate Window Tracker
    state: Arc<Mutex<HashMap<String, RateBucket>>>,
    max_requests: usize,
    window_duration: Duration,
}

impl RateLimiter {
    /// Creates a premium thread-safe Rate Limiter instance
    /// Example: `RateLimiter::new(100, Duration::from_secs(60))` allows 100 requests per minute
    pub fn new(max_requests: usize, window_duration: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window_duration,
        }
    }

    /// Validates whether the given IP is within its allowed threshold limits
    pub fn is_allowed(&self, ip: String) -> bool {
        // Acquire the thread-safe mutex guard lock safely
        let mut lock = match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(), // Handle lock recovery if a thread panicked
        };

        let now = Instant::now();

        // Lookup the client record or initialize a completely fresh one
        let bucket = lock.entry(ip).or_insert_with(|| RateBucket {
            request_count: 0,
            window_expires_at: now + self.window_duration,
        });

        // If the window has expired, reset the tracker completely for the new window block
        if now >= bucket.window_expires_at {
            bucket.request_count = 1;
            bucket.window_expires_at = now + self.window_duration;
            return true;
        }

        // If within the window, check if they have exceeded the limit thresholds
        if bucket.request_count < self.max_requests {
            bucket.request_count += 1;
            true // Access Granted!
        } else {
            false // Rate limit exceeded! Access Denied (Throws HTTP 429)
        }
    }
}
