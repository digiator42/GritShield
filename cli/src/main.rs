// gritshield_cli/src/main.rs
use clap::{Parser, Subcommand};
use dialoguer::Select;
use heck::{AsPascalCase, AsSnakeCase};
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::Write;
use std::path::Path;

mod migration_runner;

use crate::migration_runner::MigrationRunner;

#[derive(Parser)]
#[command(name = "gritshield")]
#[command(about = "🛡️  GritShield Framework Command Line Interface", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a brand new production-ready Gritshield application
    New { name: String },

    /// Generate framework structures (controllers, models, migrations)
    #[command(alias = "gen")]
    Generate {
        #[command(subcommand)]
        blueprint: Blueprints,
    },

    /// Run database migrations
    Migration {
        /// Migration direction: up or down
        #[arg(value_enum)]
        direction: MigrationDirection,
        /// Optional migration file name (defaults to latest)
        #[arg(short, long)]
        file: Option<String>,
    },
}

#[derive(Subcommand)]
enum Blueprints {
    /// Generate a fresh controller with micro-route attributes
    Controller { name: String },
    /// Generate a database model struct with structural query blocks
    Model { name: String },
    /// Generate an empty raw SQL schema migration script
    Migration { description: String },
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum MigrationDirection {
    Up,
    Down,
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::New { name } => {
            create_project(name);
        }
        Commands::Generate { blueprint } => match blueprint {
            Blueprints::Controller { name } => generate_controller(name),
            Blueprints::Model { name } => generate_model(name),
            Blueprints::Migration { description } => generate_migration(description),
        },
        Commands::Migration { direction, file } => {
            run_migration(direction, file);
        }
    }
}

// =========================================================================
// COMMAND: NEW PROJECT SCAFFOLDER
// =========================================================================
fn create_project(name: &str) {
    let base_path = Path::new(name);

    println!(
        "\x1b[36m🚀 Creating project '{}' under GritShield architecture...\x1b[0m",
        name
    );

    // 1. Interactive Database Selection Prompt
    let db_options = vec![
        "SQLite (Embedded - File based)",
        "PostgreSQL (Production)",
        "MySQL",
        "SQLite (In-Memory - No persistence)",
        "No Database (Pure API mode)",
    ];

    let db_selection = Select::new()
        .with_prompt("Choose database engine layout")
        .items(&db_options)
        .default(0)
        .interact()
        .unwrap();

    // Determine database configuration
    let (db_url, db_feature, db_driver) = match db_selection {
        0 => ("sqlite://app.db?mode=rwc", "sqlx-sqlite", "SQLite (File)"),
        1 => (
            "postgres://postgres:password@localhost:5432/app_db",
            "sqlx-postgres",
            "PostgreSQL",
        ),
        2 => (
            "mysql://root:password@127.0.0.1:3306/app_db",
            "sqlx-mysql",
            "MySQL",
        ),
        3 => ("sqlite::memory:", "sqlx-sqlite", "SQLite (In-Memory)"),
        _ => ("", "", "No Database"),
    };

    // Scaffold Directory Tree
    create_dir_all(base_path.join("src/controllers")).unwrap();
    create_dir_all(base_path.join("src/models")).unwrap();
    create_dir_all(base_path.join("migrations")).unwrap();
    create_dir_all(base_path.join("static/css")).unwrap();

    // Write .env file
    if db_selection != 4 {
        let env_content = format!(
            r#"DATABASE_URL={}
GRIT_LOG=info
APP_ENV=development
JWT_SECRET=your-secret-key-change-in-production
"#,
            db_url
        );
        write_file(&base_path.join(".env"), &env_content);
    } else {
        // No database - minimal .env
        let env_content = r#"GRIT_LOG=info
APP_ENV=development
JWT_SECRET=your-secret-key-change-in-production
"#;
        write_file(&base_path.join(".env"), &env_content);
    }

    // Write updated Cargo.toml with dynamic engine feature bindings
    let toml_package = if db_selection == 4 {
        "sea-orm = { version = \"1.0\", features = [\"runtime-tokio-native-tls\", \"macros\"] }"
    } else {
        &format!("sea-orm = {{ version = \"1.0\", features = [\"{}\", \"runtime-tokio-native-tls\", \"macros\"] }}", db_feature)
    };

    let cargo_toml = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[dependencies]
gritshield = "0.1.1"
tokio = {{ version = "1.0", features = ["full"] }}
maud = "0.25"
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
ctor = "1.0.7"
dotenvy = "0.15"
chrono = "0.4"
{}
"#,
        name, toml_package
    );
    write_file(&base_path.join("Cargo.toml"), &cargo_toml);

    // Write base controller manifest file
    write_file(&base_path.join("src/controllers/mod.rs"), "pub mod info;");

    // Write boilerplate info controller using the new impl syntax
    let info_ctrl = r#"use gritshield::prelude::*;

pub struct ApiController;

#[controller("/api")]
impl ApiController {
    
    #[get("/info")]
    pub async fn system_info(_ctx: RequestContext) -> Response {
        Response::ok("GritShield Engine Core Node Online.")
    }

    #[get("/health")]
    pub async fn health_check(_ctx: RequestContext) -> Response {
        Response::ok("OK")
    }
}
"#;
    write_file(&base_path.join("src/controllers/info.rs"), info_ctrl);

    // Write main.rs with appropriate database configuration
    let main_rs = if db_selection == 4 {
        // No database mode
        r#"
use gritshield::prelude::*;

mod controllers;

#[get("/static/:*path")]
async fn serve_static(ctx: RequestContext) -> Response {
    let path = ctx.params.get("*path").unwrap().as_str();
    Response::static_file(&format!("static/{}", path))
}

#[tokio::main]
async fn main() {
    let router = Router::new();
    run_server("127.0.0.1", "8080", router).await;
}
"#
    } else {
        // With database
        r#"
use gritshield::prelude::*;
use gritshield::security::db::{DbManager, DbConfig};
use std::sync::Arc;

mod controllers;

#[get("/")]
async fn index(_ctx: RequestContext) -> Response {
    Response::ok(Sanitizer::trust(
        "<h1>Shield Operational</h1><p>GritShield application is successfully running.</p>",
    ))
}

#[tokio::main]
async fn main() {
    // Initialize the engine configuration setup matrix
    let db_config = DbConfig::default();

    // Fire connection pool parameters and run pending dynamic migrations automatically!
    let shared_db = DbManager::connect(db_config).await.unwrap();

    // Mount database pool directly onto the context router pipeline bounds
    let router = Router::new()
        .mount_db(shared_db);

    // Run server
    run_server("127.0.0.1", "8080", router).await;
}
"#
    };
    write_file(&base_path.join("src/main.rs"), main_rs);

    // Standard styling CSS sheet
    write_file(
        &base_path.join("static/css/style.css"),
        "body { font-family: sans-serif; background: #0f172a; color: #f8fafc; padding: 2rem; }",
    );

    println!("\n\x1b[32m✨ Project setup complete! Run the following to start cooking:\x1b[0m\n");
    println!("   cd {}", name);
    println!("   cargo run\n");
    println!("   \x1b[36mUsing database: {}\x1b[0m", db_driver);
}

// =========================================================================
// 🛠️ BLUEPRINT GENERATOR HOOKS
// =========================================================================

fn generate_controller(name: &str) {
    let snake_name = format!("{}", AsSnakeCase(name));
    let pascal_name = format!("{}", AsPascalCase(name));
    let file_path = format!("src/controllers/{}.rs", snake_name);

    // Ensure controllers directory exists
    let controllers_dir = Path::new("src/controllers");
    if !controllers_dir.exists() {
        create_dir_all(controllers_dir).unwrap();
        let mod_path = controllers_dir.join("mod.rs");
        if !mod_path.exists() {
            write_file(&mod_path, "");
        }
    }

    if Path::new(&file_path).exists() {
        println!(
            "\x1b[31mError: Controller '{}' already exists.\x1b[0m",
            file_path
        );
        return;
    }

    let template = format!(
        r#"use gritshield::prelude::*;

pub struct {}Controller;

#[controller("/{}")]
impl {}Controller {{
    
    #[get("/")]
    pub async fn list(_ctx: RequestContext) -> Response {{
        Response::ok("List all {}")
    }}

    #[post("/")]
    pub async fn create(_ctx: RequestContext) -> Response {{
        Response::ok("Create new {}")
    }}

    #[get("/:id")]
    pub async fn show(ctx: RequestContext) -> Response {{
        let id = ctx.params.get("*id").unwrap();
        Response::ok(format!("Showing {{}} with id: {{}}", "{}", id))
    }}

    #[put("/:id")]
    pub async fn update(ctx: RequestContext) -> Response {{
        let id = ctx.params.get("*id").unwrap();
        Response::ok(format!("Updating {{}} with id: {{}}", "{}", id))
    }}

    #[patch("/:id")]
    pub async fn partial_update(ctx: RequestContext) -> Response {{
        let id = ctx.params.get("*id").unwrap();
        Response::ok(format!("Partially updating {{}} with id: {{}}", "{}", id))
    }}

    #[delete("/:id")]
    pub async fn delete(ctx: RequestContext) -> Response {{
        let id = ctx.params.get("*id").unwrap();
        Response::ok(format!("Deleting {{}} with id: {{}}", "{}", id))
    }}
}}
"#,
        pascal_name, // struct name
        snake_name,  // parent route
        pascal_name, // struct name
        pascal_name, // list response
        pascal_name, // create response
        pascal_name, // show response first placeholder
        pascal_name, // update response first placeholder
        pascal_name, // partial_update response first placeholder
        pascal_name, // delete response first placeholder
    );

    write_file(Path::new(&file_path), &template);
    append_mod_registration("src/controllers/mod.rs", &snake_name);
    println!(
        "\x1b[32m[SCAFFOLD] Created controller: {}\x1b[0m",
        file_path
    );
}

fn generate_model(name: &str) {
    let snake_name = format!("{}", AsSnakeCase(name));
    let pascal_name = format!("{}", AsPascalCase(name));
    let file_path = format!("src/models/{}.rs", snake_name);

    // Ensure models directory exists
    let models_dir = Path::new("src/models");
    if !models_dir.exists() {
        create_dir_all(models_dir).unwrap();
        let mod_path = models_dir.join("mod.rs");
        if !mod_path.exists() {
            write_file(&mod_path, "");
        }
    }

    if Path::new(&file_path).exists() {
        println!(
            "\x1b[31mError: Model '{}' already exists.\x1b[0m",
            file_path
        );
        return;
    }

    let template = format!(
        r#"use serde::{{Serialize, Deserialize}};
use gritshield::security::repository::GritRepository;
use sea_orm::*;
use sea_orm_migration::async_trait::async_trait;

#[derive(Debug, Serialize, Deserialize, Clone, FromQueryResult, ModelTrait)]
pub struct {} {{
    pub id: i64,
    pub created_at: i64,
    pub updated_at: i64,
}}

impl {} {{
    // Implement data map queries here
}}

#[async_trait]
impl GritRepository for {}Repository {{
    type Entity = ;
    type Model = {};
    type Column = ;
    type ActiveModel = ;

    fn get_db(&self) -> &DatabaseConnection {{
        todo!()
    }}

    fn email_column() -> Self::Column {{
        todo!()
    }}
}}
"#,
        pascal_name, pascal_name, pascal_name, pascal_name
    );

    write_file(Path::new(&file_path), &template);
    append_mod_registration("src/models/mod.rs", &snake_name);
    println!(
        "\x1b[32m[SCAFFOLD] Created data model: {}\x1b[0m",
        file_path
    );
}

fn generate_migration(description: &str) {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let slug = format!("{}", AsSnakeCase(description));
    let file_path = format!("migrations/{}_{}.sql", timestamp, slug);

    let template = format!(
        r#"-- Migration: {}
-- Created at: {}

-- Up:
-- Write your migration SQL here
-- CREATE TABLE users (
--     id INTEGER PRIMARY KEY AUTOINCREMENT,
--     email VARCHAR(255) NOT NULL UNIQUE,
--     password_hash VARCHAR(255) NOT NULL,
--     created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
-- );

-- Down:
-- DROP TABLE IF EXISTS users;
"#,
        description, timestamp
    );

    write_file(Path::new(&file_path), &template);
    println!(
        "\x1b[32m[SCAFFOLD] Generated schema migration: {}\x1b[0m",
        file_path
    );
}

// =========================================================================
// 🛠️ MIGRATION COMMAND
// =========================================================================

fn run_migration(direction: &MigrationDirection, file: &Option<String>) {
    println!("\x1b[36m🔄 Running migration: {:?}\x1b[0m", direction);

    // Load environment
    let _ = dotenvy::dotenv();

    // Get database URL
    let db_url =
        std::env::var("DATABASE_URL").unwrap_or_else(|_| "sqlite://app.db?mode=rwc".to_string());

    println!("📦 Connecting to database: {}", db_url);

    // Create tokio runtime for async operations
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");

    runtime.block_on(async {
        // Initialize migration runner
        let runner = match MigrationRunner::new(&db_url, "migrations").await {
            Ok(r) => {
                println!("✅ Connected to database successfully");
                r
            }
            Err(e) => {
                eprintln!("\x1b[31m❌ Failed to connect to database: {}\x1b[0m", e);
                return;
            }
        };

        // Execute migration based on direction
        match direction {
            MigrationDirection::Up => {
                if let Some(file_name) = file {
                    println!("⬆️ Running specific migration: {}", file_name);
                    match runner.run_up(Some(file_name)).await {
                        Ok(_) => println!("✅ Migration applied successfully: {}", file_name),
                        Err(e) => eprintln!("\x1b[31m❌ Migration failed: {}\x1b[0m", e),
                    }
                } else {
                    println!("⬆️ Running all pending migrations...");
                    match runner.run_up(None).await {
                        Ok(_) => println!("✅ All migrations applied successfully"),
                        Err(e) => eprintln!("\x1b[31m❌ Migration failed: {}\x1b[0m", e),
                    }
                }
            }
            MigrationDirection::Down => {
                if let Some(file_name) = file {
                    println!("⬇️ Rolling back specific migration: {}", file_name);
                    match runner.run_down(Some(file_name)).await {
                        Ok(_) => println!("✅ Migration rolled back successfully: {}", file_name),
                        Err(e) => eprintln!("\x1b[31m❌ Rollback failed: {}\x1b[0m", e),
                    }
                } else {
                    println!("⬇️ Rolling back last migration...");
                    match runner.run_down(None).await {
                        Ok(_) => println!("✅ Rollback completed successfully"),
                        Err(e) => eprintln!("\x1b[31m❌ Rollback failed: {}\x1b[0m", e),
                    }
                }
            }
        }
    });
}

// =========================================================================
// 🧰 HELPERS
// =========================================================================

fn write_file(path: &Path, content: &str) {
    let mut file = File::create(path).expect("Failed to create file resource target");
    file.write_all(content.as_bytes())
        .expect("Failed to build file payload string buffer");
}

fn append_mod_registration(manifest_path: &str, mod_name: &str) {
    let path = Path::new(manifest_path);
    if !path.exists() {
        write_file(path, "");
    }

    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "pub mod {};", mod_name).unwrap();
}
