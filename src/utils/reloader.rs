use notify::{RawEvent, RecursiveMode, Watcher, raw_watcher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

pub struct HotReloader;

impl HotReloader {
    pub fn start() {
        if std::env::var("RUNNING_UNDER_RELOADER").is_ok() {
            return;
        }

        // Get the directory where the developer executed 'cargo run'
        // This ensures we are always anchored to the developer's project root
        let project_root =
            std::env::current_dir().expect("Failed to get current working directory");

        println!(
            "[HOT-RELOAD] Supervisor active anchoring at: {:?}",
            project_root
        );

        let (tx, rx) = channel();
        let mut watcher = raw_watcher(tx).expect("Failed to initialize file watcher");

        // Target the developer's absolute paths explicitly
        let src_dir = project_root.join("src");
        let static_dir = project_root.join("static");

        if src_dir.exists() {
            watcher.watch(&src_dir, RecursiveMode::Recursive).unwrap();
        } else {
            println!(
                "[HOT-RELOAD] WARNING: Could not find developer 'src' at {:?}",
                src_dir
            );
        }

        if static_dir.exists() {
            watcher
                .watch(&static_dir, RecursiveMode::Recursive)
                .unwrap();
        }

        // Pass the project_root into spawn_app so cargo run knows WHERE to run
        let mut child_process = Self::spawn_app(&project_root);
        let mut last_reload = Instant::now();

        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(RawEvent {
                    path: Some(_),
                    op: Ok(op),
                    ..
                }) => {
                    if op.is_empty() || op.contains(notify::Op::CHMOD) {
                        continue;
                    }

                    if last_reload.elapsed() > Duration::from_millis(800) {
                        println!(
                            "[HOT-RELOAD] Developer file modification detected. Resetting lock..."
                        );

                        let _ = child_process.kill();
                        let _ = child_process.wait();

                        // Tiny Windows filesystem cooldown
                        std::thread::sleep(Duration::from_millis(100));

                        println!("[HOT-RELOAD] Rebuilding developer application...");
                        child_process = Self::spawn_app(&project_root);
                        last_reload = Instant::now();
                    }
                }
                _ => {}
            }
        }
    }

    // Pass the active project root directory down to the cargo process
    fn spawn_app(current_dir: &PathBuf) -> std::process::Child {
        Command::new("cargo")
            .arg("run")
            .current_dir(current_dir)
            .env("CARGO_TARGET_DIR", current_dir.join("target/reloader")) // Private target folder inside developer app
            .env("RUNNING_UNDER_RELOADER", "1")
            .spawn()
            .expect("Failed to execute application process")
    }
}
