use notify::{RawEvent, RecursiveMode, Watcher, raw_watcher};
use std::process::Command;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

pub struct HotReloader;

impl HotReloader {
    pub fn start() {
        if std::env::var("RUNNING_UNDER_RELOADER").is_ok() {
            return;
        }

        println!("[HOT-RELOAD] Supervisor active. Watching src/ and static/...");

        let (tx, rx) = channel();
        let mut watcher = raw_watcher(tx).expect("Failed to initialize file watcher");

        watcher.watch("src", RecursiveMode::Recursive).unwrap();
        watcher.watch("static", RecursiveMode::Recursive).unwrap();

        let mut child_process = Self::spawn_app();
        let mut last_reload = Instant::now();

        loop {
            // Use a slightly longer timeout for the receiver to keep CPU usage low
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(RawEvent {
                    path: Some(_),
                    op: Ok(op),
                    ..
                }) => {
                    if op.is_empty() || op.contains(notify::Op::CHMOD) {
                        continue;
                    }

                    // Debounce: Wait 800ms after the last change to ensure IDE is done writing
                    if last_reload.elapsed() > Duration::from_millis(800) {
                        println!("[HOT-RELOAD] File modification detected. Resetting lock...");

                        // 1. KILL the child
                        let _ = child_process.kill();

                        // 2. WAIT for the process to exit completely
                        // This returns the process to the OS so the .exe isn't "in use"
                        let _ = child_process.wait();

                        // 3. WINDOWS COOLDOWN
                        // Give the filesystem a moment to finalize the file handle release
                        std::thread::sleep(Duration::from_millis(200));

                        println!("[HOT-RELOAD] Rebuilding and Restarting...");
                        child_process = Self::spawn_app();
                        last_reload = Instant::now();
                    }
                }
                _ => {}
            }
        }
    }

    fn spawn_app() -> std::process::Child {
        Command::new("cargo")
            .arg("run")
            // Separate build folder for the reloader
            // to prevent "Access Denied" and "Blocking for lock" errors
            .env("CARGO_TARGET_DIR", "target/reloader")
            .env("RUNNING_UNDER_RELOADER", "1")
            .spawn()
            .expect("Failed to execute application process")
    }
}
