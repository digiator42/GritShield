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

        // Simple MIME detection
        let content_type = match path.extension().and_then(|s| s.to_str()) {
            Some("css") => "text/css",
            Some("js") => "application/javascript",
            Some("png") => "image/png",
            Some("jpg") | Some("jpeg") => "image/jpeg",
            _ => "text/plain",
        };

        Ok((content, content_type.to_string()))
    } else {
        Err("File Not Found".to_string())
    }
}
