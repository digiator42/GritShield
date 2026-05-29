use notify::{RawEvent, RecursiveMode, Watcher, raw_watcher};
use std::path::{PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

pub struct HotReloader;

impl HotReloader {
    pub fn start() {
        if std::env::var("RUNNING_UNDER_RELOADER").is_ok() {
            return;
        }

        let project_root =
            std::env::current_dir().expect("Failed to get current working directory");
        println!(
            "[HOT-RELOAD] Supervisor active anchoring at: {:?}",
            project_root
        );

        let (tx, rx) = channel();
        let mut watcher = raw_watcher(tx).expect("Failed to initialize file watcher");

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

        let mut child_process = Self::rebuild_and_spawn(&project_root, None);

        // Track when a change is pending
        let mut change_pending = false;
        let mut last_event_time = Instant::now();
        let debounce_duration = Duration::from_millis(300); // Quick, snappy 300ms debounce

        loop {
            // Poll for events with a small timeout to keep the loop driving
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(RawEvent { op: Ok(op), .. }) => {
                    // Ignore the event if it's completely empty OR if it is strictly a CHMOD event
                    if !op.is_empty() && op != notify::Op::CHMOD {
                        change_pending = true;
                        last_event_time = Instant::now();
                    }
                }
                _ => {}
            }

            // Flush out the remaining rapid event streams
            while let Ok(RawEvent { op: Ok(op), .. }) = rx.try_recv() {
                if !op.is_empty() && op != notify::Op::CHMOD {
                    change_pending = true;
                    last_event_time = Instant::now();
                }
            }

            // Trigger execution ONLY when a change is pending AND the user has stopped saving/typing
            if change_pending && last_event_time.elapsed() >= debounce_duration {
                println!(
                    "[HOT-RELOAD] triggered. Rebuilding application..."
                );
                child_process = Self::rebuild_and_spawn(&project_root, child_process);

                // Reset state state-machine variables
                change_pending = false;
            }
        }
    }

    fn rebuild_and_spawn(current_dir: &PathBuf, active_child: Option<Child>) -> Option<Child> {
        if let Some(mut child) = active_child {
            let _ = child.kill();
            let _ = child.wait();
        }

        // Essential OS filesystem handle release delay
        std::thread::sleep(Duration::from_millis(100));

        let build_status = Command::new("cargo")
            .arg("build")
            .current_dir(current_dir)
            .env("CARGO_TARGET_DIR", current_dir.join("target/reloader"))
            .env("RUNNING_UNDER_RELOADER", "1")
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();

        match build_status {
            Ok(status) if status.success() => {
                println!("[HOT-RELOAD] Build successful. Launching server executable...");

                let new_child = Command::new("cargo")
                    .args(["run", "--quiet"])
                    .current_dir(current_dir)
                    .env("CARGO_TARGET_DIR", current_dir.join("target/reloader"))
                    .env("RUNNING_UNDER_RELOADER", "1")
                    .stdout(Stdio::inherit())
                    .stderr(Stdio::inherit())
                    .spawn()
                    .expect("Failed to execute application process");

                Some(new_child)
            }
            _ => {
                eprintln!("[HOT-RELOAD] Compilation failed. Waiting for code corrections...");
                None
            }
        }
    }
}
