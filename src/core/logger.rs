use crate::core::env::get_env;
use crate::core::initialize_env;
use crate::http::request::Request;
use crate::info;
use crate::security::telemetry::TELEMETRY;
use colored::*;
use std::fmt;
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use std::thread;

// --- Log Level ---
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum LogLevel {
    Off = 0,
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

impl LogLevel {
    /// Parse from string (case‑insensitive). Returns `None` for unknown.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "off" | "false" | "0" | "disabled" => Some(Self::Off),
            "error" => Some(Self::Error),
            "warn" => Some(Self::Warn),
            "info" => Some(Self::Info),
            "debug" => Some(Self::Debug),
            "trace" => Some(Self::Trace),
            _ => None,
        }
    }

    /// Get the default log level from environment or .env file
    pub fn from_env_or_default() -> Self {
        let log_level = get_env("GRIT_LOG", "");
        if let Some(level) = Self::from_str(&log_level) {
            return level;
        }

        // Fallback to RUST_LOG for compatibility
        let rust_log = crate::core::env::get_env("RUST_LOG", "");
        if !rust_log.is_empty() {
            if let Some(level) = Self::from_str(&rust_log) {
                return level;
            }
        }

        // Default to Off if nothing or unknown string is set
        Self::Off
    }
}

// --- Asynchronous Non-Blocking Logger ---
pub struct Logger {
    pub level: LogLevel,
    sender: Sender<String>,
}

impl Logger {
    pub fn new(level: LogLevel) -> Self {
        let (tx, rx) = mpsc::channel::<String>();

        // Spawn a dedicated background OS thread to write to stderr without blocking Tokio workers
        thread::spawn(move || {
            while let Ok(msg) = rx.recv() {
                eprintln!("{}", msg);
            }
        });

        Self { level, sender: tx }
    }

    /// Log a message at the given level asynchronously
    pub fn log(&self, level: LogLevel, args: fmt::Arguments<'_>) {
        if self.level != LogLevel::Off && level <= self.level {
            let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let level_str = format!("{:?}", level);
            let colored_level = match level {
                LogLevel::Error => level_str.red().bold(),
                LogLevel::Warn => level_str.yellow().bold(),
                LogLevel::Info => level_str.cyan(),
                LogLevel::Debug => level_str.blue(),
                LogLevel::Trace => level_str.magenta(),
                LogLevel::Off => level_str.white(),
            };

            let formatted_msg = format!("[{}] {}: {}", timestamp, colored_level, args);
            let _ = self.sender.send(formatted_msg);
        }
    }
}

// Global singleton
static GLOBAL_LOGGER: OnceLock<Logger> = OnceLock::new();

pub fn init(level: LogLevel) {
    GLOBAL_LOGGER.get_or_init(|| Logger::new(level));
}

pub fn get_logger() -> &'static Logger {
    GLOBAL_LOGGER.get_or_init(|| {
        let level = LogLevel::from_env_or_default();
        Logger::new(level)
    })
}

/// Initialize the global logger from environment variables.
/// Returns `true` if logging is enabled, or `false` if set to `Off`/`false`.
pub fn init_from_env() -> bool {
    initialize_env();
    let level = LogLevel::from_env_or_default();
    init(level);

    let is_enabled = level != LogLevel::Off;

    if is_enabled {
        get_logger().log(
            LogLevel::Info,
            format_args!(
                "[KERNEL-LOGGER] Initialized logger with level '{:?}'",
                level
            ),
        );
    }

    is_enabled
}

pub fn log_request_summary(
    req: &Request,
    status: u16,
    duration: std::time::Duration,
    session_id: Option<String>,
    user_id: Option<String>,
) {
    // 1. Record metrics centrally with zero macro overhead
    TELEMETRY.record_request(&req.path, status, duration);

    // 2. Format logger output payload
    let method_str = format!("{:?}", req.method);
    let status_str = colorize_status(status);

    let duration_str = if duration.as_millis() > 0 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{}µs", duration.as_micros())
    };

    let body_size = req.body.len();
    let size_str = if body_size < 1024 {
        format!("{} B", body_size)
    } else {
        format!("{:.2} KB", body_size as f64 / 1024.0)
    };

    let identity_str = match (user_id, session_id) {
        (Some(uid), _) => format!("🔑 JWT Sub: {}", uid),
        (_, Some(sid)) => format!("🍪 Session ID: {}", &sid[..8.min(sid.len())]),
        _ => "👤 Anonymous".to_string(),
    };

    info!(
        "🗲  [{}] {} {} ➔  Size: {} | Time: {} | Auth: {}",
        status_str, method_str, req.path, size_str, duration_str, identity_str
    );
}

fn colorize_status(status: u16) -> String {
    match status {
        200..=299 => format!("{}", status).green().bold().to_string(),
        300..=399 => format!("{}", status).cyan().to_string(),
        400..=499 => format!("{}", status).red().to_string(),
        500..=599 => format!("{}", status).magenta().to_string(),
        _ => format!("{}", status).yellow().to_string(),
    }
}
