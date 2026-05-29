use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr, ConnectionTrait, Statement};
use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub async fn connect(url: &str) -> Result<DatabaseConnection, DbErr> {
    println!("[GRITSHIELD] Connecting to database at {}...", url);
    Database::connect(url).await
}

/// Framework database options wrapper to enforce structural safety
pub struct DbConfig {
    pub url: String,
    pub max_connections: u32,
    pub min_connections: u32,
    pub connect_timeout: Duration,
    pub idle_timeout: Duration,
}

impl Default for DbConfig {
    fn default() -> Self {
        // Fallback to environment variable or standard secure defaults
        let url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://app.db?mode=rwc".to_string());

        Self {
            url,
            max_connections: 20,
            min_connections: 5,
            connect_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(300),
        }
    }
}

/// Core Database Manager for GritShield
pub struct DbManager;

impl DbManager {
    /// Connects to the database using configured framework safeguards
    pub async fn connect(config: DbConfig) -> Result<DatabaseConnection, DbErr> {
        println!("[GRITSHIELD] Initializing secure database cluster link...");

        let mut opt = ConnectOptions::new(config.url);

        // Enforce connection limits and health checks automatically
        opt.max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .connect_timeout(config.connect_timeout)
            .idle_timeout(config.idle_timeout)
            .sqlx_logging(true) // Enable telemetry logging for security audits
            .sqlx_logging_level(log::LevelFilter::Debug);

        let connection = Database::connect(opt).await?;
        println!("[GRITSHIELD] Database connection pool online and verified.");

        // Framework responsibility: Run pending structural schema changes
        if let Err(e) = Self::run_pending_migrations(&connection).await {
            println!(
                "\x1b[31m[GRITSHIELD] Migration failure warning: {}\x1b[0m",
                e
            );
        }

        Ok(connection)
    }

    /// Automatically scans the local directory for migrations and applies them
    async fn run_pending_migrations(db: &DatabaseConnection) -> Result<(), String> {
        let migration_path = Path::new("migrations");
        if !migration_path.exists() {
            return Ok(()); // No migrations directory found, skip silently
        }

        println!("\x1b[34m[GRITSHIELD] Checking schema migration ledgers...\x1b[0m");
        let backend = db.get_database_backend();

        // 1. Ensure the framework's tracking ledger table exists
        let create_table_sql = match backend {
            sea_orm::DatabaseBackend::MySql => {
                "CREATE TABLE IF NOT EXISTS gritshield_migrations (
                    version VARCHAR(255) PRIMARY KEY,
                    applied_at BIGINT NOT NULL
                );"
            }
            _ => {
                "CREATE TABLE IF NOT EXISTS gritshield_migrations (
                    version TEXT PRIMARY KEY,
                    applied_at BIGINT NOT NULL
                );"
            }
        };

        db.execute(Statement::from_string(
            backend,
            create_table_sql.to_string(),
        ))
        .await
        .map_err(|e| format!("Failed to create migration ledger table: {}", e))?;

        // 2. Read and sort all migration files chronologically by timestamp
        let mut entries = fs::read_dir(migration_path)
            .map_err(|e| format!("Failed to read migrations directory: {}", e))?
            .filter_map(|res| res.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "sql"))
            .collect::<Vec<_>>();

        // Sorting by filename naturally orders them by timestamp prefix (e.g., 17152345_init.sql)
        entries.sort_by_key(|e| e.file_name());

        // 3. Process each migration file sequential
        for entry in entries {
            let file_name = entry.file_name().into_string().unwrap_or_default();

            // Check if this file version has already been executed
            let check_sql = format!(
                "SELECT 1 FROM gritshield_migrations WHERE version = '{}';",
                file_name.replace("'", "''")
            );

            let query_res = db
                .query_one(Statement::from_string(backend, check_sql))
                .await
                .map_err(|e| format!("Failed querying ledger: {}", e))?;

            if query_res.is_some() {
                // Already applied, skip to the next file
                continue;
            }

            println!(
                "\x1b[33m[MIGRATION] Applying pending delta: {}...\x1b[0m",
                file_name
            );

            // 4. Read file content and extract the script block under "-- Up:"
            let content = fs::read_to_string(entry.path())
                .map_err(|e| format!("Failed to read migration file contents: {}", e))?;

            let up_script = extract_up_sql(&content);

            if up_script.trim().is_empty() {
                println!(
                    "\x1b[35m[MIGRATION] Warning: File {} has an empty execution block.\x1b[0m",
                    file_name
                );
            } else {
                // Execute the raw schema adjustments
                db.execute(Statement::from_string(backend, up_script.clone()))
                    .await
                    .map_err(|e| {
                        format!("Migration compilation failure inside {}: {}", file_name, e)
                    })?;
            }

            // 5. Log completion inside the structural ledger table
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            let log_sql = format!(
                "INSERT INTO gritshield_migrations (version, applied_at) VALUES ('{}', {});",
                file_name.replace("'", "''"),
                now
            );

            db.execute(Statement::from_string(backend, log_sql))
                .await
                .map_err(|e| format!("Failed to log migration status for {}: {}", file_name, e))?;

            println!(
                "\x1b[32m[MIGRATION] Successfully executed: {}\x1b[0m",
                file_name
            );
        }

        println!("[GRITSHIELD] Database schema sync finalized.");
        Ok(())
    }
}

/// Helper utility to isolate the migration updates section
fn extract_up_sql(content: &str) -> String {
    let mut up_lines = Vec::new();
    let mut collecting = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Target structural marker blocks generated by your CLI template
        if trimmed.to_lowercase().contains("-- up:") || trimmed.to_lowercase().contains("-- up") {
            collecting = true;
            continue;
        }
        if trimmed.to_lowercase().contains("-- down:") || trimmed.to_lowercase().contains("-- down")
        {
            break;
        }

        if collecting {
            up_lines.push(line);
        }
    }

    // If the file does not contain clean explicit blocks, fallback to running the whole file
    if up_lines.is_empty() && !content.to_lowercase().contains("-- down") {
        return content.to_string();
    }

    up_lines.join("\n")
}
