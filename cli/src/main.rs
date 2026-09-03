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
#[command(about = "ProtectionEngine Firewall & CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a brand new production-ready Gritshield application
    New {
        /// Application name
        name: String,
        /// Enable the admin panel feature
        #[arg(short, long)]
        admin: bool,
        /// Enable the swagger/OpenAPI feature
        #[arg(short, long)]
        swagger: bool,
    },

    /// Generate framework structures (controllers, models, migrations, events, jobs, etc.)
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
        /// Optional migration file name (defaults to all)
        #[arg(short, long)]
        file: Option<String>,
    },

    /// Inspect the auto-discovered DI container and routing topology
    Diag {
        /// Export the dependency graph as Graphviz DOT
        #[arg(short, long)]
        dot: bool,
        /// Export the dependency graph as Mermaid markdown
        #[arg(short, long)]
        mermaid: bool,
    },
}

#[derive(Subcommand)]
enum Blueprints {
    /// Generate a fresh controller with micro-route attributes
    Controller { name: String },
    /// Generate a database model struct with GritModel derive and SeaORM entity
    Model { name: String },
    /// Generate a GritAdmin-annotated repository for a model
    Repository { name: String },
    /// Generate a GritComponent struct with DI wiring
    Component { name: String },
    /// Generate a GritEvent struct and an event handler
    Event { name: String },
    /// Generate a GritJob struct and a job handler with retry config
    Job { name: String },
    /// Generate an AOP interceptor struct + impl
    Interceptor { name: String },
    /// Generate a #[catch] handler for a custom HTTP status code
    Catch { status: u16 },
    /// Generate a security capabilities module (declare_security_caps!)
    Caps { name: String },
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
        Commands::New { name, admin, swagger } => {
            create_project(name, *admin, *swagger);
        }
        Commands::Generate { blueprint } => match blueprint {
            Blueprints::Controller { name } => generate_controller(name),
            Blueprints::Model { name } => generate_model(name),
            Blueprints::Repository { name } => generate_repository(name),
            Blueprints::Component { name } => generate_component(name),
            Blueprints::Event { name } => generate_event(name),
            Blueprints::Job { name } => generate_job(name),
            Blueprints::Interceptor { name } => generate_interceptor(name),
            Blueprints::Catch { status } => generate_catch(*status),
            Blueprints::Caps { name } => generate_caps(name),
            Blueprints::Migration { description } => generate_migration(description),
        },
        Commands::Migration { direction, file } => {
            run_migration(direction, file);
        }
        Commands::Diag { dot, mermaid } => {
            run_diag(*dot, *mermaid);
        }
    }
}

// =========================================================================
// COMMAND: NEW PROJECT SCAFFOLDER
// =========================================================================
fn create_project(name: &str, enable_admin: bool, enable_swagger: bool) {
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
    let (db_url, db_feature) = match db_selection {
        0 => ("sqlite://app.db?mode=rwc", "sqlite"),
        1 => ("postgres://postgres:postgres@localhost:5432/app_db", "postgres"),
        2 => ("mysql://root:root@127.0.0.1:3306/app_db", "mysql"),
        3 => ("sqlite::memory:", "sqlite"),
        _ => ("", ""),
    };

    // Scaffold Directory Tree
    create_dir_all(base_path.join("src/controllers")).unwrap();
    create_dir_all(base_path.join("src/models")).unwrap();
    create_dir_all(base_path.join("src/repositories")).unwrap();
    create_dir_all(base_path.join("src/services")).unwrap();
    create_dir_all(base_path.join("migrations")).unwrap();
    create_dir_all(base_path.join("static/css")).unwrap();
    create_dir_all(base_path.join("src/security")).unwrap();

    // Write .env file
    let mut env_lines = vec![
        "GRIT_LOG=info".to_string(),
        "APP_ENV=development".to_string(),
        "HOST=127.0.0.1".to_string(),
        "PORT=8080".to_string(),
        "JWT_SECRET=your-secret-key-change-in-production".to_string(),
    ];
    if db_selection != 4 {
        env_lines.push(format!("DATABASE_URL={}", db_url));
    }
    write_file(
        &base_path.join(".env"),
        &format!("{}\n", env_lines.join("\n")),
    );

    // Build feature list
    let mut features = Vec::new();
    if enable_admin {
        features.push("admin");
    }
    if enable_swagger {
        features.push("swagger");
    }
    let features_attr = if features.is_empty() {
        String::new()
    } else {
        format!(", features = [{}]", features.iter().map(|f| format!("\"{}\"", f)).collect::<Vec<_>>().join(", "))
    };

    // Resolve crate dep line for sea-orm with proper feature
    let sea_orm_dep = if db_selection == 4 {
        // No database – still need sea-orm for the framework but no DB driver
        "sea-orm = { version = \"1.1\", features = [\"runtime-tokio-native-tls\", \"macros\"] }"
    } else {
        &format!(
            "sea-orm = {{ version = \"1.1\", features = [\"{}\", \"runtime-tokio-native-tls\", \"macros\", \"with-chrono\", \"with-json\"] }}",
            db_feature
        )
    };

    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[dependencies]
gritshield = {{ version = "0.2.2"{features_attr} }}
tokio = {{ version = "1.0", features = ["full"] }}
maud = "0.25"
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"
ctor = "1.0"
dotenvy = "0.15"
chrono = {{ version = "0.4", features = ["serde"] }}
{sea_orm_dep}
"#,
        name = name,
        features_attr = features_attr,
        sea_orm_dep = sea_orm_dep,
    );
    write_file(&base_path.join("Cargo.toml"), &cargo_toml);

    // Write boilerplate controller manifest + info controller
    write_file(
        &base_path.join("src/controllers/mod.rs"),
        "pub mod info;\n",
    );

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

    // Write models/mod.rs
    write_file(&base_path.join("src/models/mod.rs"), "");

    // Write repositories/mod.rs
    write_file(&base_path.join("src/repositories/mod.rs"), "");

    // Write services/mod.rs
    write_file(&base_path.join("src/services/mod.rs"), "");

    // Write security/mod.rs
    write_file(&base_path.join("src/security/mod.rs"), "pub mod caps;\n");

    // Write placeholder security caps module
    write_file(
        &base_path.join("src/security/caps.rs"),
        r#"use gritshield::declare_security_caps;

// Define your capability tokens below, e.g. `pub struct ManageBilling;`

declare_security_caps! {
    // ViewDashboard => [Admin, Manager],
}
"#,
    );

    // Write main.rs with appropriate database configuration
    let main_rs = if db_selection == 4 {
        // No database mode
        r#"use gritshield::prelude::*;

mod controllers;
mod models;
mod repositories;
mod services;
mod security;

#[get("/static/:*path")]
async fn serve_static(ctx: RequestContext) -> Response {
    let path = ctx.params.get("*path").unwrap().as_str();
    Response::static_file(&format!("static/{}", path))
}

#[launch]
async fn main() {
    let router = Router::new();
    Shield::build()
        .router(router)
        .launch();
}
"#
    } else {
        // With database
        r#"use gritshield::prelude::*;
use gritshield::database::db::{DbConfig, DbManager};
use std::sync::Arc;

mod controllers;
mod models;
mod repositories;
mod services;
mod security;

#[get("/")]
async fn index(_ctx: RequestContext) -> Response {
    Response::ok(Sanitizer::trust(
        "<h1>Shield Operational</h1><p>GritShield application is successfully running.</p>",
    ))
}

#[launch]
async fn main() {
    // Initialize the engine configuration setup matrix
    let db_config = DbConfig::default();

    // Fire connection pool parameters and run pending dynamic migrations automatically!
    let shared_db = DbManager::connect(db_config).await.unwrap();

    // Mount database pool directly onto the context router pipeline bounds
    let router = Router::new()
        .mount_db(shared_db);

    // Run server
    Shield::build()
        .mount_db(shared_db)
        .router(router)
        .launch();
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
    println!("   \x1b[36mUsing database: {}\x1b[0m", db_url);
}

// =========================================================================
// 🛠️ BLUEPRINT GENERATOR HOOKS
// =========================================================================
fn generate_controller(name: &str) {
    let snake_name = format!("{}", AsSnakeCase(name));
    let pascal_name = format!("{}", AsPascalCase(name));
    // Avoid double "Controller" suffix
    let struct_name = if name.to_lowercase().ends_with("controller") {
        pascal_name.clone()
    } else {
        format!("{}Controller", pascal_name)
    };
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

pub struct {struct_name};

#[controller("/{snake}")]
impl {struct_name} {{

    #[get("/")]
    pub async fn list(_ctx: RequestContext) -> Response {{
        Response::ok("List all {pascal}")
    }}

    #[post("/")]
    pub async fn create(_ctx: RequestContext) -> Response {{
        Response::ok("Create new {pascal}")
    }}

    #[get("/:id")]
    pub async fn show(ctx: RequestContext) -> Response {{
        let id = ctx.params.get(":id").unwrap();
        Response::ok(format!("Showing {pascal} with id: {{}}", id))
    }}

    #[put("/:id")]
    pub async fn update(ctx: RequestContext) -> Response {{
        let id = ctx.params.get(":id").unwrap();
        Response::ok(format!("Updating {pascal} with id: {{}}", id))
    }}

    #[patch("/:id")]
    pub async fn partial_update(ctx: RequestContext) -> Response {{
        let id = ctx.params.get(":id").unwrap();
        Response::ok(format!("Partially updating {pascal} with id: {{}}", id))
    }}

    #[delete("/:id")]
    pub async fn delete(ctx: RequestContext) -> Response {{
        let id = ctx.params.get(":id").unwrap();
        Response::ok(format!("Deleting {pascal} with id: {{}}", id))
    }}
}}
"#,
        struct_name = struct_name,
        pascal = pascal_name,
        snake = snake_name,
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
        r#"use chrono::NaiveDateTime;
use gritshield::{{GritModel, GritRelation}};
use sea_orm::entity::prelude::*;
use serde::{{Serialize, Deserialize}};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, GritModel)]
#[sea_orm(table_name = "{snake}s")]
pub struct Model {{
    #[sea_orm(primary_key)]
    pub id: i64,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation, GritRelation)]
#[grit(table = "{snake}s")]
pub enum Relation {{}}

impl ActiveModelBehavior for ActiveModel {{}}
"#,
        snake = snake_name,
    );

    write_file(Path::new(&file_path), &template);
    append_mod_registration("src/models/mod.rs", &snake_name);
    println!(
        "\x1b[32m[SCAFFOLD] Created data model: {}\x1b[0m",
        file_path
    );
}

fn generate_repository(name: &str) {
    let snake_name = format!("{}", AsSnakeCase(name));
    // Derive the model module name: if name ends with "Repository", strip it
    let model_name = if name.to_lowercase().ends_with("repository") {
        let stripped = &name[..name.len() - "Repository".len()];
        format!("{}", AsSnakeCase(stripped))
    } else {
        snake_name.clone()
    };
    let pascal_name = format!("{}", AsPascalCase(name));
    let file_path = format!("src/repositories/{}.rs", snake_name);

    // Ensure repositories directory exists
    let repos_dir = Path::new("src/repositories");
    if !repos_dir.exists() {
        create_dir_all(repos_dir).unwrap();
        let mod_path = repos_dir.join("mod.rs");
        if !mod_path.exists() {
            write_file(&mod_path, "");
        }
    }

    if Path::new(&file_path).exists() {
        println!(
            "\x1b[31mError: Repository '{}' already exists.\x1b[0m",
            file_path
        );
        return;
    }

    let template = format!(
        r#"use crate::models::{model}::*;
use gritshield::{{database::TxnRepository, GritAdmin, GritComponent}};
use gritshield::database::repository::transaction::CURRENT_TXN;
use sea_orm::{{ActiveModelTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait}};

#[derive(Clone, GritAdmin, GritComponent)]
#[repository(
    searchable = ["id"],
    grid_columns = ["id", "created_at", "updated_at"],
    read_only = ["id", "created_at"],
)]
pub struct {pascal} {{
    pub db: DatabaseConnection,
}}

impl {pascal} {{
    pub async fn create(&self, model: ActiveModel) -> Result<Model, DbErr> {{
        if let Ok(txn) = CURRENT_TXN.try_with(|t| t.clone()) {{
            model.insert(txn.as_ref()).await
        }} else {{
            model.insert(&self.db).await
        }}
    }}

    pub async fn find_all(&self) -> Result<Vec<Model>, DbErr> {{
        Entity::find().all(&self.db).await
    }}
}}
"#,
        model = model_name,
        pascal = pascal_name,
    );

    write_file(Path::new(&file_path), &template);
    append_mod_registration("src/repositories/mod.rs", &snake_name);
    println!(
        "\x1b[32m[SCAFFOLD] Created repository: {}\x1b[0m",
        file_path
    );
}

fn generate_component(name: &str) {
    let snake_name = format!("{}", AsSnakeCase(name));
    let pascal_name = format!("{}", AsPascalCase(name));
    let file_path = format!("src/services/{}.rs", snake_name);

    // Ensure services directory exists
    let services_dir = Path::new("src/services");
    if !services_dir.exists() {
        create_dir_all(services_dir).unwrap();
        let mod_path = services_dir.join("mod.rs");
        if !mod_path.exists() {
            write_file(&mod_path, "");
        }
    }

    if Path::new(&file_path).exists() {
        println!(
            "\x1b[31mError: Component '{}' already exists.\x1b[0m",
            file_path
        );
        return;
    }

    let template = format!(
        r#"use gritshield::{{GritComponent, prelude::*}};
use std::sync::Arc;

#[derive(Clone, GritComponent)]
pub struct {pascal} {{}}

impl {pascal} {{
    pub fn new() -> Self {{
        {pascal} {{}}
    }}

    pub async fn execute(&self, ctx: RequestContext) -> Response {{
        Response::ok("Service {pascal} executed")
    }}
}}
"#,
        pascal = pascal_name,
    );

    write_file(Path::new(&file_path), &template);
    append_mod_registration("src/services/mod.rs", &snake_name);
    println!(
        "\x1b[32m[SCAFFOLD] Created component: {}\x1b[0m",
        file_path
    );
}

fn generate_event(name: &str) {
    let snake_name = format!("{}", AsSnakeCase(name));
    let pascal_name = format!("{}", AsPascalCase(name));
    let file_path = format!("src/events/{}.rs", snake_name);

    // Ensure events directory exists
    let events_dir = Path::new("src/events");
    if !events_dir.exists() {
        create_dir_all(events_dir).unwrap();
        let mod_path = events_dir.join("mod.rs");
        if !mod_path.exists() {
            write_file(&mod_path, "");
        }
    }

    if Path::new(&file_path).exists() {
        println!(
            "\x1b[31mError: Event '{}' already exists.\x1b[0m",
            file_path
        );
        return;
    }

    let template = format!(
        r#"use gritshield::{{event, GritEvent, prelude::*}};
use serde::{{Deserialize, Serialize}};
use std::sync::Arc;

#[derive(GritEvent, Clone, Serialize, Deserialize)]
pub struct {pascal} {{
    pub aggregate_id: String,
}}

pub struct {pascal}Handler;

#[event]
impl {pascal}Handler {{
    pub async fn handle(&self, event: Arc<{pascal}>) {{
        // Handle the event here (e.g., send email, emit notifications)
        println!("Handling event for: {{}}", event.aggregate_id);
    }}
}}
"#,
        pascal = pascal_name,
    );

    write_file(Path::new(&file_path), &template);
    append_mod_registration("src/events/mod.rs", &snake_name);
    println!(
        "\x1b[32m[SCAFFOLD] Created event: {}\x1b[0m",
        file_path
    );
}

fn generate_job(name: &str) {
    let snake_name = format!("{}", AsSnakeCase(name));
    let pascal_name = format!("{}", AsPascalCase(name));
    let file_path = format!("src/jobs/{}.rs", snake_name);

    // Ensure jobs directory exists
    let jobs_dir = Path::new("src/jobs");
    if !jobs_dir.exists() {
        create_dir_all(jobs_dir).unwrap();
        let mod_path = jobs_dir.join("mod.rs");
        if !mod_path.exists() {
            write_file(&mod_path, "");
        }
    }

    if Path::new(&file_path).exists() {
        println!(
            "\x1b[31mError: Job '{}' already exists.\x1b[0m",
            file_path
        );
        return;
    }

    let template = format!(
        r#"use gritshield::{{job, GritJob}};
use serde::{{Deserialize, Serialize}};
use std::time::Duration;

#[derive(Serialize, Deserialize, GritJob)]
pub struct {pascal} {{
    pub payload: String,
}}

#[job(retries = 3)]
impl {pascal} {{
    pub async fn perform(&self) -> Result<(), String> {{
        // Implement your background job logic here
        println!("Running job with payload: {{}}", self.payload);
        Ok(())
    }}
}}
"#,
        pascal = pascal_name,
    );

    write_file(Path::new(&file_path), &template);
    append_mod_registration("src/jobs/mod.rs", &snake_name);
    println!(
        "\x1b[32m[SCAFFOLD] Created job: {}\x1b[0m",
        file_path
    );
}

fn generate_interceptor(name: &str) {
    let snake_name = format!("{}", AsSnakeCase(name));
    let pascal_name = format!("{}", AsPascalCase(name));
    let file_path = format!("src/interceptors/{}.rs", snake_name);

    // Ensure interceptors directory exists
    let interceptors_dir = Path::new("src/interceptors");
    if !interceptors_dir.exists() {
        create_dir_all(interceptors_dir).unwrap();
        let mod_path = interceptors_dir.join("mod.rs");
        if !mod_path.exists() {
            write_file(&mod_path, "");
        }
    }

    if Path::new(&file_path).exists() {
        println!(
            "\x1b[31mError: Interceptor '{}' already exists.\x1b[0m",
            file_path
        );
        return;
    }

    let template = format!(
        r#"use gritshield::core::aop::{{Interceptor, InvocationContext, BoxFuture}};
use sea_orm::{{DatabaseConnection, DbErr}};
use sea_orm_migration::async_trait::async_trait;
use std::time::Instant;

pub struct {pascal};

#[async_trait]
impl Interceptor for {pascal} {{
    async fn intercept<'a>(
        &'a self,
        ctx: InvocationContext<'a>,
        next: Box<dyn FnOnce() -> BoxFuture<'a, Result<(), DbErr>> + Send + 'a>,
    ) -> Result<(), DbErr> {{
        let start = Instant::now();
        println!("[{pascal}] Before: {{}}::{{}}", ctx.target_name, ctx.method_name);

        let result = next().await;

        let elapsed = start.elapsed();
        match &result {{
            Ok(_) => println!("[{pascal}] After: {{}}::{{}} completed in {{:?}}", ctx.target_name, ctx.method_name, elapsed),
            Err(e) => println!("[{pascal}] After: {{}}::{{}} failed in {{:?}} - {{}}", ctx.target_name, ctx.method_name, elapsed, e),
        }}

        result
    }}
}}
"#,
        pascal = pascal_name,
    );

    write_file(Path::new(&file_path), &template);
    append_mod_registration("src/interceptors/mod.rs", &snake_name);
    println!(
        "\x1b[32m[SCAFFOLD] Created interceptor: {}\x1b[0m",
        file_path
    );
}

fn generate_catch(status: u16) {
    let file_path = format!("src/catch.rs");

    if Path::new(&file_path).exists() {
        println!(
            "\x1b[31mError: catch.rs already exists. Manually add your #[catch] handler to: {}\x1b[0m",
            file_path
        );
        return;
    }

    let template = format!(
        r#"use gritshield::catch;
use gritshield::prelude::*;

#[catch(status = {status})]
pub async fn handle_{status_lower}(ctx: RequestContext) -> Response {{
    Response::{resp_method}(format!("{{}}", "Custom error page for {status}"))
}}
"#,
        status = status,
        status_lower = status.to_string().to_lowercase(),
        resp_method = match status {
            404 => "not_found",
            500 => "internal_error",
            403 => "forbidden",
            401 => "unauthorized",
            400 => "bad_request",
            _ => "not_found",
        },
    );

    write_file(Path::new(&file_path), &template);
    println!(
        "\x1b[32m[SCAFFOLD] Created catch handler: {}\x1b[0m",
        file_path
    );
}

fn generate_caps(name: &str) {
    let file_path = "src/security/caps.rs";

    // Ensure security directory exists
    let security_dir = Path::new("src/security");
    if !security_dir.exists() {
        create_dir_all(security_dir).unwrap();
    }

    let template = format!(
        r#"use gritshield::declare_security_caps;

// Capability tokens — declare your security primitives here.
// Each capability is backed by a zero-sized marker type.
// Capabilities are verified at compile time via #[cap(...)] on route handlers.

pub struct Admin;
pub struct Manager;
pub struct Editor;
pub struct Viewer;

// Your new capability token
pub struct {pascal};

declare_security_caps! {{
    Admin    => [Admin],
    {pascal} => [Admin, Manager],
}}
"#,
        pascal = AsPascalCase(name),
    );

    write_file(Path::new(file_path), &template);
    println!(
        "\x1b[32m[SCAFFOLD] Created security caps module: {}\x1b[0m",
        file_path
    );
}

fn generate_migration(description: &str) {
    let timestamp = chrono::Local::now()
        .format("%Y%m%d_%H%M%S")
        .to_string();

    let slug = format!("{}", AsSnakeCase(description));
    let file_path = format!("migrations/{}_{}.sql", timestamp, slug);

    let template = format!(
        r#"-- Migration: {desc}
-- Created at: {ts}

-- Up:
CREATE TABLE IF NOT EXISTS {table} (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

-- Down:
DROP TABLE IF EXISTS {table};
"#,
        desc = description,
        ts = timestamp,
        table = slug,
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
// 🔍 DIAGNOSTICS COMMAND
// =========================================================================
fn run_diag(dot: bool, mermaid: bool) {
    println!("\x1b[36m🔍 GritShield Diagnostics\x1b[0m\n");

    if dot {
        println!("\x1b[33m[DOT] Dependency Injection Graph (Graphviz)\x1b[0m");
        println!("   Run this in your project context where gritshield is a dependency:");
        println!("     use gritshield::core::ioc::AutoWire;");
        println!("     print!(\"{{}}\", AutoWire::export_dot());");
    }

    if mermaid {
        println!("\x1b[33m[MERMAID] Dependency Injection Graph (Markdown)\x1b[0m");
        println!("   Run this in your project context where gritshield is a dependency:");
        println!("     use gritshield::core::ioc::AutoWire;");
        println!("     print!(\"{{}}\", AutoWire::export_mermaid());");
    }

    if !dot && !mermaid {
        println!("   Available diagnostic exporters:");
        println!("     --dot      Export dependency graph as Graphviz DOT");
        println!("     --mermaid  Export dependency graph as Mermaid markdown");
    }
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

    // Read existing content to check if mod already registered
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let registration = format!("pub mod {};", mod_name);
    if existing.contains(&registration) {
        return;
    }

    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    writeln!(file, "pub mod {};", mod_name).unwrap();
}
