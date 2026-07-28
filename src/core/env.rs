use colored::*;
use std::sync::Once;

use crate::{info, warn};

static INIT_ENV: Once = Once::new();

/// Automatically initializes the .env configuration system exactly once on boot.
/// Subsequent calls bypass the disk completely for optimal memory speeds.
pub fn initialize_env() {
    INIT_ENV.call_once(|| {
        if dotenvy::dotenv().is_ok() {
            println!("-===?>???");
            info!(
                "{}",
                "[CONFIG] Successfully loaded .env configurations into memory."
                    .bold()
                    .green()
            );
        } else {
            println!("-===?>???");
            warn!(
                "{}",
                "[CONFIG] No .env file detected. Falling back to system environment variables."
                    .bold()
                    .yellow()
            );
        }
    });
}

/// An environment variable getter with strict hierarchical system lookups and defaults.
pub fn get_env(key: &str, default: &str) -> String {
    // Query process memory. std::env::var automatically checks BOTH:
    //    a) Values injected via dotenvy from a local file
    //    b) Actual native OS system variables (e.g. DATABASE_URL set via bash/docker env flags)
    match std::env::var(key) {
        Ok(val) => val, // Found in file or OS
        Err(_) => {
            // fallback to default
            default.to_string()
        }
    }
}
