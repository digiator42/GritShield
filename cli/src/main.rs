// gritshield/src/main.rs
use clap::{Parser, Subcommand};
use dialoguer::{Select};
use heck::{AsPascalCase, AsSnakeCase};
use std::fs::{File, OpenOptions, create_dir_all};
use std::io::Write;
use std::path::Path;

#[derive(Parser)]
#[command(name = "gritshield")]
#[command(about = "🛡️ GritShield Framework Command Line Interface", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a brand new production-ready Gritshield application
    New { name: String }, //

    /// Generate framework structures (controllers, models, migrations)
    #[command(alias = "g")]
    Generate {
        #[command(subcommand)]
        blueprint: Blueprints,
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

fn main() {
    let cli = Cli::parse(); //

    match &cli.command {
        //
        Commands::New { name } => {
            //
            create_project(name); //
        }
        Commands::Generate { blueprint } => match blueprint {
            Blueprints::Controller { name } => generate_controller(&name),
            Blueprints::Model { name } => generate_model(&name),
            Blueprints::Migration { description } => generate_migration(&description),
        },
    }
}

// =========================================================================
// 🚀 COMMAND: NEW PROJECT SCAFFOLDER
// =========================================================================
fn create_project(name: &str) {
    //
    let base_path = Path::new(name); //

    println!(
        "\x1b[36m🚀 Creating project '{}' under GritShield architecture...\x1b[0m",
        name
    ); //

    // 1. Interactive Database Selection Prompt
    let db_options = vec!["SQLite (Embedded)", "PostgreSQL (Production)", "MySQL"];
    let db_selection = Select::new()
        .with_prompt("Choose database engine layout")
        .items(&db_options)
        .default(0)
        .interact()
        .unwrap();

    let db_url = match db_selection {
        0 => "sqlite://app.db?mode=rwc",
        1 => "postgres://postgres:password@localhost:5432/app_db",
        _ => "mysql://root:password@127.0.0.1:3306/app_db",
    };

    // Scaffold Directory Tree
    create_dir_all(base_path.join("src/controllers")).unwrap();
    create_dir_all(base_path.join("src/models")).unwrap();
    create_dir_all(base_path.join("migrations")).unwrap();
    create_dir_all(base_path.join("static/css")).unwrap(); //

    // Write updated Cargo.toml referencing your new public crates.io version!
    let cargo_toml = format!(
r#"[package]
name = "{}"
version = "0.1.0"
edition = "2024"

[dependencies]
gritshield = "0.1.0"
tokio = {{ version = "1.0", features = ["full"] }}
maud = "0.25"
serde = {{ version = "1.0", features = ["derive"] }}
"#,
        name
    );
    write_file(&base_path.join("Cargo.toml"), &cargo_toml);

    // Write base controller manifest file
    write_file(&base_path.join("src/controllers/mod.rs"), "pub mod info;");

    // Write boilerplate info controller
    let info_ctrl = r#"use gritshield::prelude::*;

#[get("/api/info")]
pub async fn system_info(_ctx: RequestContext) -> Response {
    Response::ok("GritShield Engine Core v0.1.0 Node Online.")
}
"#;
    write_file(&base_path.join("src/controllers/info.rs"), info_ctrl);

    // Write main.rs with dynamic pipeline mounts
    let main_rs = format!(
        r#"mod controllers;
mod models;

use gritshield::prelude::*;
use std::sync::Arc;

#[get("/")]
async fn index(_ctx: RequestContext) -> Response {{
    Response::new(200, Sanitizer::trust("<h1>Shield Operational</h1><p>GritShield application is successfully running.</p>"))
}}

#[tokio::main]
async fn main() {{
    let db = gritshield::security::db::connect("{}").await.unwrap();
    let shared_db = Arc::new(db);

    let router = Router::new();

    println!("\x1b[32m[GRITSHIELD] Booting cluster link on http://127.0.0.1:8080\x1b[0m");
    gritshield::core::server::run_server("127.0.0.1", "8080", router, shared_db, true).await;
}}
"#,
        db_url
    );
    write_file(&base_path.join("src/main.rs"), &main_rs); //

    // Standard styling CSS sheet
    write_file(
        &base_path.join("static/css/style.css"),
        "body { font-family: sans-serif; background: #0f172a; color: #f8fafc; padding: 2rem; }",
    ); //

    println!("\n\x1b[32m✨ Project setup complete! Run the following to start cooking:\x1b[0m\n"); //
    println!("   cd {}", name); //
    println!("   cargo run\n"); //
}

// =========================================================================
// 🛠️ BLUEPRINT GENERATOR HOOKS
// =========================================================================

fn generate_controller(name: &str) {
    let snake_name = format!("{}", AsSnakeCase(name));
    let file_path = format!("src/controllers/{}.rs", snake_name);

    if Path::new(&file_path).exists() {
        println!(
            "\x1b[31mError: Controller '{}' already exists.\x1b[0m",
            file_path
        );
        return;
    }

    let template = format!(
        r#"use gritshield::prelude::*;

#[get("/{}")]
pub async fn list(_ctx: RequestContext) -> Response {{
    Response::ok("List data for {}")
}}

#[post("/{}")]
pub async fn create(_ctx: RequestContext) -> Response {{
    Response::ok("Entity created.")
}}
"#,
        snake_name, snake_name, snake_name
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

    if Path::new(&file_path).exists() {
        println!(
            "\x1b[31mError: Model '{}' already exists.\x1b[0m",
            file_path
        );
        return;
    }

    let template = format!(
        r#"use serde::{{Serialize, Deserialize}};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct {} {{
    pub id: i64,
    pub created_at: i64,
}}

impl {} {{
    // Implement data map queries here
}}
"#,
        pascal_name, pascal_name
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

-- -- Up: Write execution updates here
-- CREATE TABLE sample (id INTEGER PRIMARY KEY);

-- -- Down: Write rollback steps here
-- DROP TABLE sample;
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
// 🧰 HELPERS
// =========================================================================
fn write_file(path: &Path, content: &str) {
    //
    let mut file = File::create(path).expect("Failed to create file resource target"); //
    file.write_all(content.as_bytes())
        .expect("Failed to build file payload string buffer"); //
} //

fn append_mod_registration(manifest_path: &str, mod_name: &str) {
    let path = Path::new(manifest_path);
    // Create manifest file if missing
    if !path.exists() {
        write_file(path, "");
    }

    let mut file = OpenOptions::new().append(true).open(path).unwrap();

    writeln!(file, "pub mod {};", mod_name).unwrap();
}
