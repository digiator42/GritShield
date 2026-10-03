use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// How many invocations are retained in memory for the admin inspector.
const DEFAULT_CAPACITY: usize = 512;

/// One recorded MCP interaction.
///
/// Every field is populated whether the call succeeded or not. An audit trail
/// that only logs the happy path is not an audit trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpAuditEntry {
    pub id: u64,
    /// RFC 3339 timestamp.
    pub timestamp: String,
    /// The JSON-RPC method that was invoked.
    pub method: String,
    /// Tool name, or the resource URI / prompt name for non-tool calls.
    pub target: String,
    /// Authenticated subject, or `anonymous`.
    pub caller: String,
    /// The role resolved at invocation time.
    pub role: Option<String>,
    pub client_ip: String,
    pub session_id: Option<String>,
    /// The arguments, truncated so one verbose call cannot flood the buffer.
    pub parameters: Value,
    /// `ok`, `denied`, `invalid`, `failed` or `rejected`.
    pub outcome: String,
    /// Optional detail for a non-`ok` outcome.
    pub detail: Option<String>,
    pub duration_micros: u64,
}

impl McpAuditEntry {
    pub fn is_success(&self) -> bool {
        self.outcome == "ok"
    }
}

/// The outcome labels used by the inspector's colour coding.
pub mod outcome {
    pub const OK: &str = "ok";
    pub const DENIED: &str = "denied";
    pub const INVALID: &str = "invalid";
    pub const FAILED: &str = "failed";
    pub const REJECTED: &str = "rejected";
}

/// A bounded, in-memory ring buffer of recent MCP invocations.
///
/// Deliberately in-process and bounded: MCP traffic is agent-driven rather than
/// user-driven, so an unbounded log is an easy memory-exhaustion vector for a
/// prompt-injected client looping on a failing tool.
pub struct McpAuditLog {
    entries: Mutex<VecDeque<McpAuditEntry>>,
    sequence: AtomicU64,
    capacity: usize,
}

impl McpAuditLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: Mutex::new(VecDeque::with_capacity(capacity.min(64))),
            sequence: AtomicU64::new(1),
            capacity,
        }
    }

    pub fn record(
        &self,
        method: &str,
        target: &str,
        caller: &str,
        role: Option<String>,
        client_ip: &str,
        session_id: Option<String>,
        parameters: Value,
        outcome: &str,
        detail: Option<String>,
        duration_micros: u64,
    ) -> u64 {
        let id = self.sequence.fetch_add(1, Ordering::Relaxed);
        let entry = McpAuditEntry {
            id,
            timestamp: chrono::Local::now().to_rfc3339(),
            method: method.to_string(),
            target: target.to_string(),
            caller: caller.to_string(),
            role,
            client_ip: client_ip.to_string(),
            session_id,
            parameters: truncate(parameters),
            outcome: outcome.to_string(),
            detail,
            duration_micros,
        };

        if let Ok(mut buffer) = self.entries.lock() {
            // `>=` rather than `>` so shrinking the capacity actually evicts.
            if buffer.len() >= self.capacity {
                buffer.pop_front();
            }
            buffer.push_back(entry);
        }

        id
    }

    /// Most recent entries first.
    pub fn recent(&self, limit: usize) -> Vec<McpAuditEntry> {
        let Ok(buffer) = self.entries.lock() else {
            return Vec::new();
        };

        buffer.iter().rev().take(limit).cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|b| b.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&self) {
        if let Ok(mut buffer) = self.entries.lock() {
            buffer.clear();
        }
    }

    /// Aggregate counters for the admin dashboard header.
    pub fn stats(&self) -> AuditStats {
        let (total, failures, denied, total_micros) = self
            .entries
            .lock()
            .map(|buffer| {
                let mut failures = 0usize;
                let mut denied = 0usize;
                let mut total_micros = 0u64;
                for entry in buffer.iter() {
                    total_micros = total_micros.saturating_add(entry.duration_micros);
                    match entry.outcome.as_str() {
                        outcome::FAILED | outcome::INVALID => failures += 1,
                        outcome::DENIED | outcome::REJECTED => denied += 1,
                        _ => {}
                    }
                }
                (buffer.len(), failures, denied, total_micros)
            })
            .unwrap_or((0, 0, 0, 0));

        AuditStats {
            total,
            failures,
            denied,
            average_duration_micros: if total == 0 {
                0
            } else {
                total_micros / total as u64
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AuditStats {
    pub total: usize,
    pub failures: usize,
    pub denied: usize,
    pub average_duration_micros: u64,
}

/// Cap a recorded argument payload so the ring buffer stays bounded in *bytes*
/// as well as in entries.
fn truncate(mut value: Value) -> Value {
    const MAX_STRING: usize = 512;

    fn walk(node: &mut Value, depth: usize) {
        // Bound nesting so a pathological payload cannot stack-overflow the
        // recursive walker.
        if depth > 32 {
            *node = Value::String("<nesting limit exceeded>".to_string());
            return;
        }

        match node {
            Value::String(text) => {
                if text.chars().count() > MAX_STRING {
                    let head: String = text.chars().take(MAX_STRING).collect();
                    *text = format!("{}…[truncated]", head);
                }
            }
            Value::Array(items) => {
                // Keep at most 50 elements; the rest are summarized.
                if items.len() > 50 {
                    let overflow = items.len() - 50;
                    items.truncate(50);
                    items.push(Value::String(format!("…[{} more items]", overflow)));
                }
                for item in items.iter_mut() {
                    walk(item, depth + 1);
                }
            }
            Value::Object(map) => {
                if map.len() > 50 {
                    let overflow = map.len() - 50;
                    let doomed: Vec<String> = map.keys().take(overflow).cloned().collect();
                    for key in doomed {
                        map.remove(&key);
                    }
                    map.insert(
                        "…".to_string(),
                        Value::String(format!("[{} more keys truncated]", overflow)),
                    );
                }
                for child in map.values_mut() {
                    walk(child, depth + 1);
                }
            }
            _ => {}
        }
    }

    walk(&mut value, 0);
    value
}

lazy_static::lazy_static! {
    static ref MCP_AUDIT: McpAuditLog = McpAuditLog::new(DEFAULT_CAPACITY);
}

/// The process-wide audit log.
pub fn audit() -> &'static McpAuditLog {
    &MCP_AUDIT
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn log_with(capacity: usize) -> McpAuditLog {
        McpAuditLog::new(capacity)
    }

    #[test]
    fn ring_buffer_evicts_the_oldest_entry() {
        let log = log_with(3);
        for index in 0..5 {
            log.record(
                "tools/call",
                &format!("tool_{}", index),
                "u1",
                None,
                "127.0.0.1",
                None,
                json!({}),
                outcome::OK,
                None,
                10,
            );
        }

        assert_eq!(log.len(), 3);
        let entries = log.recent(10);
        assert_eq!(entries[0].target, "tool_4");
        assert_eq!(entries[2].target, "tool_2");
    }

    #[test]
    fn stats_separate_failures_from_denials() {
        let log = log_with(16);
        log.record("tools/call", "a", "u", None, "ip", None, json!({}), outcome::OK, None, 100);
        log.record("tools/call", "b", "u", None, "ip", None, json!({}), outcome::FAILED, None, 300);
        log.record("tools/call", "c", "u", None, "ip", None, json!({}), outcome::DENIED, None, 100);

        let stats = log.stats();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.failures, 1);
        assert_eq!(stats.denied, 1);
        assert_eq!(stats.average_duration_micros, 166);
    }

    #[test]
    fn oversized_payloads_are_truncated() {
        let log = log_with(4);
        log.record(
            "tools/call",
            "big",
            "u",
            None,
            "ip",
            None,
            json!({ "blob": "x".repeat(5000) }),
            outcome::OK,
            None,
            1,
        );

        let entry = &log.recent(1)[0];
        let blob = entry.parameters["blob"].as_str().unwrap();
        assert!(blob.len() < 600, "payload was not truncated: {}", blob.len());
        assert!(blob.contains("truncated"));
    }

    #[test]
    fn ids_are_monotonic() {
        let log = log_with(8);
        let first = log.record("m", "t", "u", None, "ip", None, json!({}), outcome::OK, None, 1);
        let second = log.record("m", "t", "u", None, "ip", None, json!({}), outcome::OK, None, 1);
        assert!(second > first);
    }
}