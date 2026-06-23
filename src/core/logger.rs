use crate::protocol::request::Request;
use colored::*;
use std::fmt;
use std::sync::OnceLock;

// Log Level
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
            "off" => Some(Self::Off),
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
        // This automatically initializes dotenvy via initialize_env()
        let log_level = crate::core::env::get_env("GRIT_LOG", "");

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

        // Default to Info if nothing is set
        Self::Info
    }
}

// Logger
pub struct Logger {
    level: LogLevel,
}

impl Logger {
    pub fn new(level: LogLevel) -> Self {
        Self { level }
    }

    /// Log a message at the given level, but only if that level is enabled.
    pub fn log(&self, level: LogLevel, args: fmt::Arguments<'_>) {
        if level <= self.level {
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
            // Use stderr so stdout remains clean for e.g. raw HTTP responses.
            eprintln!("[{}] {}: {}", timestamp, colored_level, args);
        }
    }
}

// Global singleton
static GLOBAL_LOGGER: OnceLock<Logger> = OnceLock::new();

/// Initialize the global logger. Call this once early in `main`.
/// If the logger is already set, this does nothing.
pub fn init(level: LogLevel) {
    GLOBAL_LOGGER.get_or_init(|| Logger::new(level));
}

/// Get a reference to the global logger. If not initialized, creates one with default level.
pub fn get_logger() -> &'static Logger {
    GLOBAL_LOGGER.get_or_init(|| {
        // When creating a default logger, also try to read from environment
        let level = LogLevel::from_env_or_default();
        Logger::new(level)
    })
}

/// Initialize the global logger from environment variables.
/// This is the recommended way to initialize the logger.
pub fn init_from_env() {
    let level = LogLevel::from_env_or_default();
    // Initialize the logger FIRST
    init(level);
    // THEN log the message (now the logger is properly initialized)
    // Use eprintln directly to avoid any potential recursion issues
    eprintln!(
        "[{}] {}: [KERNEL-LOGGER] init logger with level '{:?}'",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        format!("{:?}", level).blue(),
        level
    );
}

// Public logger macros
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => {
        $crate::core::logger::get_logger().log(
            $crate::core::logger::LogLevel::Error,
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {
        $crate::core::logger::get_logger().log(
            $crate::core::logger::LogLevel::Warn,
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => {
        $crate::core::logger::get_logger().log(
            $crate::core::logger::LogLevel::Info,
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        $crate::core::logger::get_logger().log(
            $crate::core::logger::LogLevel::Debug,
            format_args!($($arg)*)
        )
    };
}

#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        $crate::core::logger::get_logger().log(
            $crate::core::logger::LogLevel::Trace,
            format_args!($($arg)*)
        )
    };
}

pub fn log_request_summary(
    req: &Request,
    status: u16,
    duration: std::time::Duration,
    session_id: Option<String>,
    user_id: Option<String>,
) {
    let method_str = format!("{:?}", req.method);
    let status_str = colorize_status(status);

    // Format duration nicely depending on speed
    let duration_str = if duration.as_millis() > 0 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{}µs", duration.as_micros())
    };

    // Readable payload weight calc
    let body_size = req.body.len();
    let size_str = if body_size < 1024 {
        format!("{} B", body_size)
    } else {
        format!("{:.2} KB", body_size as f64 / 1024.0)
    };

    // Identity marker assembly
    let identity_str = match (user_id, session_id) {
        (Some(uid), _) => format!("🔑 JWT Sub: {}", uid),
        (_, Some(sid)) => format!("🍪 Session ID: {}", &sid[..8]),
        _ => "👤 Anonymous".to_string(),
    };

    // Info level is good for request summaries as they're important operational metrics
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
        _ => format!("{}", status).yellow().to_string(),
    }
}
