//! A shared, in-process activity log.
//!
//! Background work is the hard part of any example to *verify*: the interesting
//! output happens after the response was sent, so `curl` alone proves nothing.
//! Everything here - event handlers, jobs, the cron scheduler - appends a line
//! to this buffer, and `GET /api/activity` hands the whole thing back. That way
//! the guide can be checked from a terminal instead of trusted.

use std::sync::{Mutex, OnceLock};

const CAPACITY: usize = 200;

fn buffer() -> &'static Mutex<Vec<String>> {
    static BUFFER: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    BUFFER.get_or_init(|| Mutex::new(Vec::new()))
}

fn stamp() -> String {
    chrono::Local::now().format("%H:%M:%S%.3f").to_string()
}

/// Append one line. Called from handlers, jobs and the scheduler alike.
pub fn record(who: &str, message: impl AsRef<str>) {
    let line = format!("[{}] {:<14} {}", stamp(), who, message.as_ref());
    let mut buf = buffer().lock().unwrap_or_else(|e| e.into_inner());
    buf.push(line.clone());
    // Keep the buffer bounded; a long-running demo must not grow without end.
    if buf.len() > CAPACITY {
        let overflow = buf.len() - CAPACITY;
        buf.drain(0..overflow);
    }
    println!("{}", line);
}

/// The whole log, oldest first.
pub fn snapshot() -> Vec<String> {
    buffer()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Drop everything. Used by the `DELETE /api/activity` route so you can watch
/// one clean run.
pub fn clear() {
    buffer().lock().unwrap_or_else(|e| e.into_inner()).clear();
}
