use std::sync::OnceLock;
use colored::Colorize;

pub struct AdminCredentials {
    pub username: String,
    pub password: String,
}

pub fn get_admin_credentials() -> &'static AdminCredentials {
    static CREDENTIALS: OnceLock<AdminCredentials> = OnceLock::new();

    CREDENTIALS.get_or_init(|| {
        let env_user = crate::core::env::get_env("GRITSHIELD_ADMIN_USER", "");
        let env_pass = crate::core::env::get_env("GRITSHIELD_ADMIN_PASSWORD", "");
        let is_dev = crate::core::env::get_env("APP_ENV", "development") != "production";

        // Check if both user and password were explicitly set
        if !env_user.is_empty() && !env_pass.is_empty() {
            AdminCredentials {
                username: env_user,
                password: env_pass,
            }
        } else {
            // Fallback generation for dev environment
            let gen_user = if env_user.is_empty() {
                "admin".to_string()
            } else {
                env_user
            };
            
            // Generate a random 12-character security secret (or UUID)
            let gen_pass = uuid::Uuid::new_v4().to_string()[..12].to_string();

            if is_dev {
                println!("\n{}", "=========================================================".yellow().bold());
                println!(" 🔐 {}", "GRITSHIELD ONE-TIME ADMIN CREDENTIALS GENERATED".yellow().bold());
                println!(" 👤 {}", format!("Username: {}", gen_user).cyan().bold());
                println!(" 🔑 {}", format!("Password: {}", gen_pass).green().bold());
                println!(" ⚠️  {}", "To persist across restarts, set GRITSHIELD_ADMIN_USER and GRITSHIELD_ADMIN_PASSWORD in .env".dimmed());
                println!("{}\n", "=========================================================".yellow().bold());
            }

            AdminCredentials {
                username: gen_user,
                password: gen_pass,
            }
        }
    })
}