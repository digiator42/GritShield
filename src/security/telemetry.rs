use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use once_cell::sync::Lazy;

#[derive(Debug, Default)]
pub struct EndpointMetrics {
    pub total_requests: AtomicU64,
    pub errors_4xx: AtomicU64,
    pub errors_5xx: AtomicU64,
    pub total_duration_micros: AtomicU64,
}

#[derive(Debug, Clone)]
pub struct SystemTelemetry {
    pub active_connections: Arc<AtomicU64>,
    pub total_blocked_ips: Arc<AtomicU64>,
    pub total_rate_limited_reqs: Arc<AtomicU64>,
    pub total_allowed_reqs: Arc<AtomicU64>,
    pub total_errors_4xx: Arc<AtomicU64>,
    pub total_errors_5xx: Arc<AtomicU64>,
    pub total_duration_micros: Arc<AtomicU64>,
    pub endpoints: Arc<RwLock<HashMap<String, Arc<EndpointMetrics>>>>,
}

impl SystemTelemetry {
    pub fn new() -> Self {
        Self {
            active_connections: Arc::new(AtomicU64::new(0)),
            total_blocked_ips: Arc::new(AtomicU64::new(0)),
            total_rate_limited_reqs: Arc::new(AtomicU64::new(0)),
            total_allowed_reqs: Arc::new(AtomicU64::new(0)),
            total_errors_4xx: Arc::new(AtomicU64::new(0)),
            total_errors_5xx: Arc::new(AtomicU64::new(0)),
            total_duration_micros: Arc::new(AtomicU64::new(0)),
            endpoints: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Opens a tracked connection slot.
    pub fn open_connection(&self) {
        self.active_connections.fetch_add(1, Ordering::Relaxed);
    }

    /// Closes a tracked connection slot.
    ///
    /// Saturating on purpose: the gauge is a diagnostic, and a plain
    /// `fetch_sub` on an already-zero counter wraps to `u64::MAX`, which then
    /// reports an absurd active-connection count for the rest of the process
    /// lifetime. Any imbalance between opens and closes is a bug worth seeing
    /// as "stuck at 0", not as four quintillion sockets.
    pub fn close_connection(&self) {
        self.active_connections
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
                Some(count.saturating_sub(1))
            })
            .ok();
    }

    /// Record request execution from logger.rs
    pub fn record_request(&self, path: &str, status: u16, duration: Duration) {
        self.total_allowed_reqs.fetch_add(1, Ordering::Relaxed);
        let micros = duration.as_micros() as u64;
        self.total_duration_micros.fetch_add(micros, Ordering::Relaxed);

        let is_4xx = (400..500).contains(&status);
        let is_5xx = status >= 500;

        if is_4xx {
            self.total_errors_4xx.fetch_add(1, Ordering::Relaxed);
        } else if is_5xx {
            self.total_errors_5xx.fetch_add(1, Ordering::Relaxed);
        }

        // Endpoint specific breakdown
        if let Ok(endpoints) = self.endpoints.read() {
            if let Some(metric) = endpoints.get(path) {
                metric.total_requests.fetch_add(1, Ordering::Relaxed);
                metric.total_duration_micros.fetch_add(micros, Ordering::Relaxed);
                if is_4xx { metric.errors_4xx.fetch_add(1, Ordering::Relaxed); }
                if is_5xx { metric.errors_5xx.fetch_add(1, Ordering::Relaxed); }
                return;
            }
        }

        // Lazy initialization for new paths
        if let Ok(mut endpoints) = self.endpoints.write() {
            let metric = endpoints.entry(path.to_string()).or_insert_with(|| {
                Arc::new(EndpointMetrics::default())
            });
            metric.total_requests.fetch_add(1, Ordering::Relaxed);
            metric.total_duration_micros.fetch_add(micros, Ordering::Relaxed);
            if is_4xx { metric.errors_4xx.fetch_add(1, Ordering::Relaxed); }
            if is_5xx { metric.errors_5xx.fetch_add(1, Ordering::Relaxed); }
        }
    }

    /// Generate Prometheus exposition format
    pub fn export_prometheus(&self) -> String {
        let mut out = String::new();
        let total_reqs = self.total_allowed_reqs.load(Ordering::Relaxed);
        let total_4xx = self.total_errors_4xx.load(Ordering::Relaxed);
        let total_5xx = self.total_errors_5xx.load(Ordering::Relaxed);
        let active_conn = self.active_connections.load(Ordering::Relaxed);
        let blocked = self.total_blocked_ips.load(Ordering::Relaxed);
        let rate_limited = self.total_rate_limited_reqs.load(Ordering::Relaxed);

        out.push_str("# HELP grit_requests_total Total allowed HTTP requests processed\n");
        out.push_str(&format!("grit_requests_total {}\n", total_reqs));
        out.push_str(&format!("grit_errors_4xx_total {}\n", total_4xx));
        out.push_str(&format!("grit_errors_5xx_total {}\n", total_5xx));
        out.push_str(&format!("grit_active_connections {}\n", active_conn));
        out.push_str(&format!("grit_blocked_ips_total {}\n", blocked));
        out.push_str(&format!("grit_rate_limited_requests_total {}\n", rate_limited));

        if let Ok(endpoints) = self.endpoints.read() {
            for (path, metric) in endpoints.iter() {
                let reqs = metric.total_requests.load(Ordering::Relaxed);
                let dur = metric.total_duration_micros.load(Ordering::Relaxed);
                let avg_ms = if reqs > 0 { (dur as f64 / reqs as f64) / 1000.0 } else { 0.0 };

                out.push_str(&format!("grit_endpoint_requests_total{{path=\"{}\"}} {}\n", path, reqs));
                out.push_str(&format!("grit_endpoint_latency_ms{{path=\"{}\"}} {:.2}\n", path, avg_ms));
            }
        }
        out
    }
}

// Global Singleton for kernel access
pub static TELEMETRY: Lazy<SystemTelemetry> = Lazy::new(SystemTelemetry::new);