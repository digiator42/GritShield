use std::fs;
use std::path::Path;

pub fn serve_static(file_path: &str) -> Result<(Vec<u8>, String), String> {
    // Prevent Directory Traversal Attacks (e.g., /static/../../etc/passwd)
    if file_path.contains("..") {
        return Err("404 Not Found".to_string());
    }

    let full_path = format!("./{}", file_path);
    let path = Path::new(&full_path);

    if path.exists() && path.is_file() {
        let content = fs::read(path).map_err(|_| "Read Error")?;

        // Content types for the assets a modern front end actually loads.
        // `wasm` is the one that cannot be approximated: a browser will only
        // stream-compile a module served as `application/wasm`, and falls back
        // to `text/plain` failing with an opaque stream error rather than a
        // clear one.
        let content_type = match path.extension().and_then(|s| s.to_str()) {
            Some("css") => "text/css",
            Some("js") | Some("mjs") => "text/javascript",
            Some("json") => "application/json",
            Some("html") | Some("htm") => "text/html; charset=utf-8",
            Some("wasm") => "application/wasm",
            Some("svg") => "image/svg+xml",
            Some("ico") => "image/x-icon",
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            Some("gif") => "image/gif",
            Some("webp") => "image/webp",
            Some("woff2") => "font/woff2",
            Some("woff") => "font/woff",
            Some("txt") => "text/plain; charset=utf-8",
            Some("map") => "application/json",
            _ => "application/octet-stream",
        };

        Ok((content, content_type.to_string()))
    } else {
        Err("File Not Found".to_string())
    }
}
