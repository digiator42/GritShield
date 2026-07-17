use crate::routing::engine::Router;
use crate::routing::file_system::FILE_ROUTING_REGISTRY;
use crate::info;
use colored::*;
use std::fs;
use std::path::Path;

fn method_color(method: &str) -> colored::ColoredString {
    match method {
        "GET" => method.green(),
        "POST" => method.blue(),
        "PUT" => method.yellow(),
        "DELETE" => method.red(),
        "PATCH" => method.magenta(),
        _ => method.white(),
    }
}


impl Router {

    /// Seamlessly crawls a filesystem folder, computes URL paths,
    /// and mounts handlers dynamically.
    pub fn mount_file_routes<P: AsRef<Path>>(
        mut self,
        folder_path: P,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let base_path = folder_path.as_ref().to_path_buf();
        self.crawl_directory(&base_path, &base_path)?;
        Ok(self)
    }

    fn crawl_directory(&mut self, current_dir: &Path, base_dir: &Path) -> std::io::Result<()> {
        if current_dir.is_dir() {
            for entry in fs::read_dir(current_dir)? {
                let entry = entry?;
                let path = entry.path();

                if path.file_name().map_or(false, |name| name == "404.rs") {
                    // Skip it! We attach it explicitly as an engine fallback instead
                    continue;
                }

                if path.is_dir() {
                    // Recursively crawl nested folders (e.g., pages/api)
                    self.crawl_directory(&path, &base_dir)?;
                } else if path.is_file() && path.extension().map_or(false, |ext| ext == "rs") {
                    self.process_page_file(&path, base_dir);
                }
            }
        }
        Ok(())
    }

    // Update file-system crawlers to pass down macro role parameters
    fn process_page_file(&mut self, file_path: &Path, base_dir: &Path) {
        let file_key = file_path.to_string_lossy().replace("\\", "/");
        let relative = file_path.strip_prefix(base_dir).unwrap().with_extension("");
        let relative_str = relative.to_string_lossy().replace("\\", "/");

        let mut url_route = if relative_str == "index" {
            "/".to_string()
        } else if relative_str.ends_with("/index") {
            format!("/{}", relative_str.trim_end_matches("/index"))
        } else {
            format!("/{}", relative_str)
        };

        if url_route.contains('[') && url_route.contains(']') {
            url_route = url_route
                .replace("[..", ":*")
                .replace("[", ":*")
                .replace("]", "");
        }
        if url_route.contains('_') {
            url_route = url_route.replace("_", ":*");
        }

        if let Ok(registry) = FILE_ROUTING_REGISTRY.lock() {
            if let Some(registered) = registry.get(&file_key) {
                info!(
                    "[FBS-ROUTER] >>: {:<30} {} [{:<6}] {}",
                    file_key,
                    format!("->").green(),
                    method_color(&format!("{:?}", registered.method)),
                    url_route
                );
                let handler_instance = (registered.handler_factory)();

                // Seamless integration: file-system pages now pipe their macro roles straight into the trie!
                self.add_route(
                    registered.method,
                    &url_route,
                    handler_instance,
                    registered.required_role,
                );
            }
        }
    }

}