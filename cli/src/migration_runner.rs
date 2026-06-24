// gritshield_cli/src/migration_runner.rs
use sea_orm::{ConnectOptions, Database, DatabaseConnection, Statement, ConnectionTrait};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

pub struct MigrationRunner {
    db: DatabaseConnection,
    migrations_path: String,
}

impl MigrationRunner {
    pub async fn new(
        db_url: &str,
        migrations_path: &str,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut opt = ConnectOptions::new(db_url.to_string());
        opt.sqlx_logging(false);

        let db = Database::connect(opt).await?;

        // Create migration ledger table
        let backend = db.get_database_backend();
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
        .await?;

        Ok(Self {
            db,
            migrations_path: migrations_path.to_string(),
        })
    }

    pub async fn run_up(
        &self,
        specific_file: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let path = Path::new(&self.migrations_path);
        if !path.exists() {
            return Err("Migrations directory not found".into());
        }

        let backend = self.db.get_database_backend();
        let mut entries = fs::read_dir(path)?
            .filter_map(|res| res.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "sql"))
            .collect::<Vec<_>>();

        entries.sort_by_key(|e| e.file_name());

        let mut applied_count = 0;

        for entry in entries {
            let file_name = entry.file_name().into_string().unwrap_or_default();

            if let Some(specific) = specific_file {
                if file_name != specific {
                    continue;
                }
            }

            // Check if already applied
            let check_sql = format!(
                "SELECT 1 FROM gritshield_migrations WHERE version = '{}'",
                file_name.replace("'", "''")
            );

            let exists = self
                .db
                .query_one(Statement::from_string(backend, check_sql))
                .await?
                .is_some();

            if exists {
                println!("⏭️  Migration already applied: {}", file_name);
                continue;
            }

            println!("⬆️  Applying migration: {}", file_name);

            let content = fs::read_to_string(entry.path())?;
            let up_script = extract_section(&content, "Up");

            if up_script.trim().is_empty() {
                println!("⚠️  Migration has empty Up section: {}", file_name);
            } else {
                // Execute each statement separately
                for statement in up_script.split(';').filter(|s| !s.trim().is_empty()) {
                    self.db
                        .execute(Statement::from_string(backend, statement.to_string()))
                        .await?;
                }
            }

            // Log migration
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();

            let log_sql = format!(
                "INSERT INTO gritshield_migrations (version, applied_at) VALUES ('{}', {})",
                file_name.replace("'", "''"),
                now
            );

            self.db
                .execute(Statement::from_string(backend, log_sql))
                .await?;
            println!("✅  Migration applied: {}", file_name);
            applied_count += 1;
        }

        if applied_count == 0 && specific_file.is_none() {
            println!("✅ No pending migrations to apply");
        }

        Ok(())
    }

    pub async fn run_down(
        &self,
        specific_file: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let path = Path::new(&self.migrations_path);
        if !path.exists() {
            return Err("Migrations directory not found".into());
        }

        let backend = self.db.get_database_backend();
        let mut entries = fs::read_dir(path)?
            .filter_map(|res| res.ok())
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "sql"))
            .collect::<Vec<_>>();

        // Sort in reverse order for down migrations
        entries.sort_by(|a, b| b.file_name().cmp(&a.file_name()));

        let mut rolled_back_count = 0;

        for entry in entries {
            let file_name = entry.file_name().into_string().unwrap_or_default();

            if let Some(specific) = specific_file {
                if file_name != specific {
                    continue;
                }
            }

            // Check if this migration has been applied
            let check_sql = format!(
                "SELECT 1 FROM gritshield_migrations WHERE version = '{}'",
                file_name.replace("'", "''")
            );

            let exists = self
                .db
                .query_one(Statement::from_string(backend, check_sql))
                .await?
                .is_some();

            if !exists {
                println!("⏭️  Migration not applied: {}", file_name);
                continue;
            }

            println!("⬇️  Rolling back migration: {}", file_name);

            let content = fs::read_to_string(entry.path())?;
            let down_script = extract_section(&content, "Down");

            if down_script.trim().is_empty() {
                println!("⚠️  Migration has empty Down section: {}", file_name);
            } else {
                // Execute each statement separately
                for statement in down_script.split(';').filter(|s| !s.trim().is_empty()) {
                    self.db
                        .execute(Statement::from_string(backend, statement.to_string()))
                        .await?;
                }
            }

            // Remove from ledger
            let delete_sql = format!(
                "DELETE FROM gritshield_migrations WHERE version = '{}'",
                file_name.replace("'", "''")
            );

            self.db
                .execute(Statement::from_string(backend, delete_sql))
                .await?;
            println!("✅  Migration rolled back: {}", file_name);
            rolled_back_count += 1;
        }

        if rolled_back_count == 0 && specific_file.is_none() {
            println!("✅ No migrations to rollback");
        }

        Ok(())
    }
}

fn extract_section(content: &str, section: &str) -> String {
    let mut lines = Vec::new();
    let mut collecting = false;
    let target = format!("-- {}", section);
    let end_marker = if section == "Up" {
        "-- Down:"
    } else {
        "-- End"
    };

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.to_lowercase().contains(&target.to_lowercase()) {
            collecting = true;
            continue;
        }

        if collecting && trimmed.to_lowercase().contains(&end_marker.to_lowercase()) {
            break;
        }

        if collecting {
            lines.push(line);
        }
    }

    lines.join("\n")
}
