use clap::{Parser, Subcommand};
use std::fs::{File, create_dir_all};
use std::io::Write;
use std::path::Path;

#[derive(Parser)]
#[command(name = "gritshield")]
#[command(about = "Gritshield Framework Command Line Interface", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a brand new production-ready Gritshield application
    New { name: String },
}

fn main() {
    let cli = Cli::parse();

    match &cli.command {
        Commands::New { name } => {
            create_project(name);
        }
    }
}

fn create_project(name: &str) {
    let base_path = Path::new(name);

    println!(
        "🚀 Creating luxury project '{}' under Gritshield architecture...",
        name
    );

    // Scaffold Directory Tree
    create_dir_all(base_path.join("src/templates")).unwrap();
    create_dir_all(base_path.join("static")).unwrap();

    // Write Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "{}"
version = "0.1.0"
edition = "2024"

[dependencies]
gritshield = {{ path = "../gritshield" }} # Pointing locally for now, change to version later
tokio = {{ version = "1.0", features = ["full"] }}
maud = "0.25"
"#,
        name
    );
    write_file(&base_path.join("Cargo.toml"), &cargo_toml);

    // Write src/templates/layout.rs
    let layout_rs = r#"use gritshield::prelude::*;

pub fn main_layout(title: &str, content: maud::Markup) -> maud::Markup {
    html! {
        (maud::DOCTYPE)
        html {
            head {
                meta charset="utf-8";
                title { (title) }
                link rel="stylesheet" href="/static/css/style.css";
            }
            body {
                nav {
                    div class="brand" { "🛡️ Gritshield App" }
                    a href="/" { "Home" }
                }
                main class="container" {
                    (content)
                }
                footer {
                    p { "Gritshield Web Engine" }
                }
            }
        }
    }
}
"#;
    write_file(&base_path.join("src/templates/layout.rs"), layout_rs);

    // Write src/templates/mod.rs
    let templates_mod = r#"pub mod layout;"#;
    write_file(&base_path.join("src/templates/mod.rs"), templates_mod);

    // Write src/main.rs (with static file route + minimal index)
    let main_rs = r#"mod templates;
use gritshield::prelude::*;

#[get("/")]
async fn index(_ctx: RequestContext) -> Response {
    render!("Welcome Home", html! {
        h1 { "Victory!" }
        p { "Your application is successfully running under the Gritshield kernel." }
    })
}

#[get("/static/:*path")]
async fn static_assets(ctx: RequestContext) -> Response {
    let path = ctx.params.get("*path").unwrap().as_str();

    let full_fs_path = format!("static/{}", path);

    Response::static_file(&full_fs_path)
}

#[tokio::main]
async fn main() {
    // Standard SQLite connection
    let db = gritshield::security::db::connect("sqlite://app.db?mode=rwc").await.unwrap();
    let shared_db = Arc::new(db);

    let router = Router::new();

    println!("[GRITSHIELD] Booting engine cluster...");
    gritshield::core::server::run_server("127.0.0.1", "8080", router, shared_db, true).await;
}
"#;
    write_file(&base_path.join("src/main.rs"), main_rs);

    // Write a boilerplate static/css/style.css file
    let style_css = r#"body {
    font-family: system-ui, -apple-system, sans-serif;
    background: #0f172a;
    color: #f8fafc;
    margin: 0;
    padding: 2rem;
}
.container {
    max-width: 800px;
    margin: 0 auto;
    background: #1e293b;
    padding: 2rem;
    border-radius: 12px;
    box-shadow: 0 4px 6px -1px rgb(0 0 0 / 0.1);
}
nav {
    display: flex;
    align-items: center;
    max-width: 90%;
    margin: auto;
    margin-bottom: 10px;
    padding: 1rem 1.5rem;
    background: linear-gradient(135deg, #0ea5e9 0%, #2563eb 100%);
    border-radius: 8px;
    gap: 1.5rem;
}
.nav-links {
    display: flex;
    gap: 1rem;
    margin-left: auto;
}
nav a {
    color: #ffffff;
    font-weight: 500;
    transition: opacity 0.2s;
}
nav a:hover {
    opacity: 0.8;
}

footer { margin-top: 2rem; text-align: center; color: #64748b; font-size: 0.875rem; }
"#;
    write_file(&base_path.join("static/css/style.css"), style_css);

    println!("✨ Project setup complete! Run the following to start cooking:\n");
    println!("   cd {}", name);
    println!("   cargo run\n");
}

fn write_file(path: &Path, content: &str) {
    let mut file = File::create(path).expect("Failed to create file resource target");
    file.write_all(content.as_bytes())
        .expect("Failed to build file payload string buffer");
}
