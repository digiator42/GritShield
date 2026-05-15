use crate::protocol::request::Request;
use colored::*;

pub fn log_request_summary(
    req: &Request,
    status: u16,
    duration: std::time::Duration,
    session_id: Option<String>,
    user_id: Option<String>,
) {
    let method_str = format!("{:?}", req.method); //
    let status_str = colorize_status(status);

    // Format duration nicely depending on speed
    let duration_str = if duration.as_millis() > 0 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{}µs", duration.as_micros())
    };

    // Readable payload weight calc
    let body_size = req.body.len(); //
    let size_str = if body_size < 1024 {
        format!("{} B", body_size)
    } else {
        format!("{:.2} KB", body_size as f64 / 1024.0)
    };

    // Identity marker assembly
    let identity_str = match (user_id, session_id) {
        (Some(uid), _) => format!("🔑 JWT Sub: {}", uid),
        (_, Some(sid)) => format!("🍪 Session ID: {}", &sid[..8]), // show prefix
        _ => "👤 Anonymous".to_string(),
    };

    // Print a luxury, standardized log block
    println!(
        "🗲  [{}] {} {} ➔ Size: {} | Time: {} | Auth: {}",
        status_str,
        method_str,
        req.path, //
        size_str,
        duration_str,
        identity_str
    );
}

fn colorize_status(status: u16) -> String {
    // You can use standard ANSI escape codes or your color crate (like colored)
    match status {
        200..=299 => format!("{}", status).green().bold().to_string(), // Green
        300..=399 => format!("{}", status).cyan().to_string(),         // Cyan
        400..=499 => format!("{}", status).yellow().to_string(),       // Yellow
        _ => format!("{}", status).red().to_string(),                  // Red
    }
}
