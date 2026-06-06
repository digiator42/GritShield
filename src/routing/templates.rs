// 📁 In your framework: src/html/templates.rs
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::core::env::get_env;

static TEMPLATE_CACHE: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

// Keeps track of the custom root directory if overridden
static TEMPLATE_ROOT_DIR: Lazy<Mutex<PathBuf>> =
    Lazy::new(|| Mutex::new(PathBuf::from("templates")));

/// Retrieves an HTML template from the application's template directory.
///
/// In development mode, this reads directly from the disk to allow hot-reloading.
/// In production, it fetches the view instantly out of high-speed memory cache.
pub fn get_template(template: &str) -> String {
    TemplateEngine::get(template)
}
pub struct TemplateEngine;

impl TemplateEngine {
    /// Set a custom root folder if running from an unconventional directory layout
    pub fn set_root<P: AsRef<Path>>(path: P) {
        if let Ok(mut root) = TEMPLATE_ROOT_DIR.lock() {
            *root = path.as_ref().to_path_buf();
        }
    }

    pub fn precompile_all<P: AsRef<Path>>(dir_path: P) -> std::io::Result<()> {
        let base_path = dir_path.as_ref().to_path_buf();
        // Update the root tracking layout so runtime matches compilation locations
        Self::set_root(&base_path);

        if !base_path.exists() {
            return Ok(());
        }
        Self::crawl_and_cache(&base_path, &base_path)?;
        Ok(())
    }

    fn crawl_and_cache(current_dir: &Path, base_dir: &Path) -> std::io::Result<()> {
        for entry in fs::read_dir(current_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                Self::crawl_and_cache(&path, base_dir)?;
            } else if path.is_file() && path.extension().map_or(false, |e| e == "html") {
                let relative = path.strip_prefix(base_dir).unwrap();
                let template_key = relative.to_string_lossy().replace("\\", "/").to_lowercase();
                let content = fs::read_to_string(&path)?;

                if let Ok(mut cache) = TEMPLATE_CACHE.lock() {
                    cache.insert(template_key, content);
                }
            }
        }
        Ok(())
    }

    pub fn get(template_name: &str) -> String {
        let is_production = get_env("SHIELD_ENV", "development") == "production";
        let normalized_key = template_name.replace("\\", "/").to_lowercase();

        if is_production {
            if let Ok(cache) = TEMPLATE_CACHE.lock() {
                if let Some(cached_html) = cache.get(&normalized_key) {
                    return cached_html.clone();
                }
            }
            format!("<h1>Template '{}' Not Found</h1>", normalized_key)
        } else {
            // Development Mode: Build path relative to the runtime configuration base path
            let mut file_path = if let Ok(root) = TEMPLATE_ROOT_DIR.lock() {
                root.clone()
            } else {
                PathBuf::from("templates")
            };

            for segment in normalized_key.split('/') {
                if !segment.is_empty() {
                    file_path.push(segment);
                }
            }

            match fs::read_to_string(&file_path) {
                Ok(html) => html,
                Err(e) => {
                    eprintln!(
                        "[TEMPLATE ERROR] Failed to read disk asset '{}': {}",
                        file_path.display(),
                        e
                    );
                    format!("<h1>Template Read Failure: {}</h1>", normalized_key)
                }
            }
        }
    }
}
