use std::collections::HashMap;
use std::time::Duration;

use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::protocol::form::{FormData, UploadedFile};
use crate::security::xss::UntrustedString;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    PATCH,
    DELETE,
    OPTIONS,
    HEAD,
    UNKNOWN,
}
#[derive(Debug, Clone)]
pub struct Request {
    pub method: HttpMethod,
    pub path: String,
    pub uri: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub query: HashMap<String, UntrustedString>,
}

impl Request {
    pub fn new() -> Self {
        Request {
            method: HttpMethod::GET,
            path: String::new(),
            uri: String::new(),
            headers: HashMap::new(),
            body: Vec::new(),
            query: HashMap::new(),
        }
    }

    pub async fn parse(stream: &mut TcpStream) -> Result<Self, String> {
        const MAX_REQUEST_SIZE: usize = 1024 * 1024; // 1MB

        let mut buffer = vec![0; MAX_REQUEST_SIZE];

        let bytes_read = match timeout(Duration::from_secs(10), stream.read(&mut buffer)).await {
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(format!("I/O Error: {}", e)),
            Err(_) => return Err("Request timeout exceeded".to_string()),
        };

        if bytes_read == 0 {
            return Err("Empty request".to_string());
        }

        let request_raw = String::from_utf8_lossy(&buffer[..bytes_read]);

        let mut sections = request_raw.split("\r\n\r\n");

        let header_section = sections
            .next()
            .ok_or_else(|| "Malformed request".to_string())?;

        let body_section = sections.next().unwrap_or("");

        let mut lines = header_section.lines();

        // Parse request line
        let request_line = lines
            .next()
            .ok_or_else(|| "Missing request line".to_string())?;

        let parts: Vec<&str> = request_line.split_whitespace().collect();

        if parts.len() < 3 {
            return Err("Malformed request line".to_string());
        }

        let method = match parts[0] {
            "GET" => HttpMethod::GET,
            "POST" => HttpMethod::POST,
            "PUT" => HttpMethod::PUT,
            "PATCH" => HttpMethod::PATCH,
            "DELETE" => HttpMethod::DELETE,
            "OPTIONS" => HttpMethod::OPTIONS,
            "HEAD" => HttpMethod::HEAD,
            _ => HttpMethod::UNKNOWN,
        };

        let full_path = parts[1].to_string();

        let mut query_params = HashMap::new();

        let path = if let Some((base_path, query_str)) = full_path.split_once('?') {
            for pair in query_str.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    query_params.insert(k.to_string(), UntrustedString::new(v.to_string()));
                }
            }

            base_path.to_string()
        } else {
            full_path
        };

        let uri = path.clone();

        // Parse headers
        let mut headers = HashMap::new();

        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }

        // Parse body
        let body = body_section.as_bytes().to_vec();

        Ok(Request {
            method,
            path,
            uri,
            headers,
            body,
            query: query_params,
        })
    }

    pub fn parse_json_body<T: serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        let content_type = self
            .headers
            .get("content-type")
            .ok_or_else(|| "Missing Content-Type header".to_string())?;

        if !content_type.starts_with("application/json") {
            return Err("Unsupported Media Type: Expected application/json".to_string());
        }

        if self.body.is_empty() {
            return Err("Empty request body".to_string());
        }

        serde_json::from_slice(&self.body)
            .map_err(|e| format!("JSON Malformed Payload Error: {}", e))
    }

    pub fn parse_form_body(&self) -> FormData {
        let mut form_data = FormData::new();

        let content_type = match self.headers.get("content-type") {
            Some(ct) => ct,
            None => return form_data,
        };

        if content_type.starts_with("application/x-www-form-urlencoded") {
            if let Ok(body_str) = std::str::from_utf8(&self.body) {
                for pair in body_str.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        form_data
                            .fields
                            .insert(k.to_string(), UntrustedString::new(v.to_string()));
                    }
                }
            }
        }
        // multipart/form-data (Contains text fields AND binary files)
        else if content_type.starts_with("multipart/form-data") {
            // Extract the boundary sequence identifier
            if let Some(boundary_idx) = content_type.find("boundary=") {
                let boundary = format!("--{}", &content_type[boundary_idx + 9..]);
                let boundary_bytes = boundary.as_bytes();

                // Split the body into chunks using the boundary delimiter
                // (Using a basic byte-window matching approach)
                let mut parts = Vec::new();
                let mut start = 0;

                while let Some(pos) = self.body[start..]
                    .windows(boundary_bytes.len())
                    .position(|w| w == boundary_bytes)
                {
                    let end = start + pos;
                    if end > start {
                        parts.push(&self.body[start..end]);
                    }
                    start = end + boundary_bytes.len();
                }

                for part in parts {
                    // Strip leading/trailing CRLF characters safely
                    if part.is_empty() || part == b"\r\n" || part == b"--\r\n" {
                        continue;
                    }

                    // Separate the part headers from its data block via \r\n\r\n
                    if let Some(header_end) = part.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header_part = &part[..header_end];
                        let data_part = &part[header_end + 4..];

                        // Clean up trailing CRLF from the parsed body segment
                        let final_data = if data_part.ends_with(b"\r\n") {
                            &data_part[..data_part.len() - 2]
                        } else {
                            data_part
                        };

                        if let Ok(header_str) = std::str::from_utf8(header_part) {
                            let mut name = String::new();
                            let mut filename = None;
                            let mut part_content_type = "text/plain".to_string();

                            for line in header_str.lines() {
                                if line.to_lowercase().starts_with("content-disposition:") {
                                    // Extract name="xyz"
                                    if let Some(n_idx) = line.find("name=\"") {
                                        let remainder = &line[n_idx + 6..];
                                        if let Some(end_idx) = remainder.find('"') {
                                            name = remainder[..end_idx].to_string();
                                        }
                                    }
                                    // Extract filename="abc.png" if present
                                    if let Some(f_idx) = line.find("filename=\"") {
                                        let remainder = &line[f_idx + 10..];
                                        if let Some(end_idx) = remainder.find('"') {
                                            filename = Some(remainder[..end_idx].to_string());
                                        }
                                    }
                                } else if line.to_lowercase().starts_with("content-type:") {
                                    part_content_type = line[13..].trim().to_string();
                                }
                            }

                            if !name.is_empty() {
                                if let Some(fname) = filename {
                                    // It's a file!
                                    form_data.files.insert(
                                        name,
                                        UploadedFile {
                                            filename: fname,
                                            content_type: part_content_type,
                                            data: final_data.to_vec(),
                                        },
                                    );
                                } else {
                                    // It's a standard string text field
                                    if let Ok(val_str) = std::str::from_utf8(final_data) {
                                        form_data.fields.insert(
                                            name,
                                            UntrustedString::new(val_str.to_string()),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        form_data
    }

    pub fn has_header(&self, key: &str) -> bool {
        self.headers.contains_key(&key.to_lowercase())
    }
}
