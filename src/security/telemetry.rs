use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone)]
pub struct SystemTelemetry {
    pub active_connections: Arc<AtomicU64>,
    pub total_blocked_ips: Arc<AtomicU64>,
    pub total_rate_limited_reqs: Arc<AtomicU64>,
    pub total_allowed_reqs: Arc<AtomicU64>,
}

impl SystemTelemetry {
    pub fn new() -> Self {
        Self {
            active_connections: Arc::new(AtomicU64::new(0)),
            total_blocked_ips: Arc::new(AtomicU64::new(0)),
            total_rate_limited_reqs: Arc::new(AtomicU64::new(0)),
            total_allowed_reqs: Arc::new(AtomicU64::new(0)),
        }
    }
}
