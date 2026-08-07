use std::collections::HashMap;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::timeout;

use crate::http::form::{FormData, UploadedFile};
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
    pub headers: HashMap<String, Vec<String>>,
    pub body: Vec<u8>,
    pub query: HashMap<String, Vec<UntrustedString>>,
}

/// Returns immediately without allocation if input contains no '%' or '+'.
pub fn percent_decode(input: &str) -> String {
    if !input.contains('%') && !input.contains('+') {
        return input.to_string();
    }
    let mut out = Vec::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(h) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                out.push(h);
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| input.to_string())
}

pub fn sanitize_path(path: &str) -> String {
    // Fast path: if path lacks '.' or '//', no canonicalization is needed
    if !path.contains('.') && !path.contains("//") {
        return path.to_string();
    }

    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => continue,
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }

    format!("/{}", segments.join("/"))
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

    pub fn fill(
        method: HttpMethod,
        path: String,
        uri: String,
        headers: HashMap<String, Vec<String>>,
        body: Vec<u8>,
        query: HashMap<String, Vec<UntrustedString>>,
    ) -> Self {
        Request {
            method,
            path,
            uri,
            headers,
            body,
            query,
        }
    }

    pub async fn parse(stream: &mut TcpStream, buffer: &mut Vec<u8>) -> Result<Self, String> {
        const MAX_REQUEST_SIZE: usize = 1024 * 1024;
        const GROWTH_STEP: usize = 64 * 1024;

        let mut total_read = 0usize;
        let mut header_end = None;

        loop {
            if total_read == buffer.len() {
                if buffer.len() >= MAX_REQUEST_SIZE {
                    return Err("Request too large".to_string());
                }
                let new_len = (buffer.len() + GROWTH_STEP).min(MAX_REQUEST_SIZE);
                buffer.resize(new_len, 0);
            }

            let n = match timeout(Duration::from_secs(10), stream.read(&mut buffer[total_read..])).await {
                Ok(Ok(n)) => n,
                Ok(Err(e)) => return Err(format!("I/O Error: {}", e)),
                Err(_) => return Err("Request timeout exceeded after 10 seconds".to_string()),
            };

            if n == 0 {
                if total_read == 0 {
                    return Err("Empty request".to_string());
                }
                break;
            }

            total_read += n;

            if header_end.is_none() {
                if let Some(pos) = buffer[..total_read].windows(4).position(|w| w == b"\r\n\r\n") {
                    header_end = Some(pos);
                }
            }

            if let Some(h_end) = header_end {
                let mut content_length = 0usize;
                let mut is_expect_continue = false;

                if let Ok(h_str) = std::str::from_utf8(&buffer[..h_end]) {
                    for line in h_str.lines().skip(1) {
                        let line_lower = line.to_ascii_lowercase();
                        if line_lower.starts_with("content-length:") {
                            if let Some((_, v)) = line.split_once(':') {
                                content_length = v.trim().parse().unwrap_or(0);
                            }
                        } else if line_lower.starts_with("expect:") && line_lower.contains("100-continue") {
                            is_expect_continue = true;
                        }
                    }
                }

                // Handle Expect: 100-continue (1C)
                if is_expect_continue {
                    let _ = stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n").await;
                }

                if total_read >= h_end + 4 + content_length {
                    break;
                }
            }
        }

        let bytes_read = total_read;
        let h_end = header_end.ok_or_else(|| "Malformed request".to_string())?;

        let header_section = std::str::from_utf8(&buffer[..h_end])
            .map_err(|_| "Invalid UTF-8 in headers".to_string())?;

        let body_bytes = if bytes_read > h_end + 4 {
            buffer[h_end + 4..bytes_read].to_vec()
        } else {
            Vec::new()
        };

        let mut lines = header_section.lines();
        let request_line = lines.next().ok_or_else(|| "Missing request line".to_string())?;
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

        let full_path = parts[1];
        let mut query_params: HashMap<String, Vec<UntrustedString>> = HashMap::new();

        let raw_path = if let Some((base_path, query_str)) = full_path.split_once('?') {
            for pair in query_str.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    let key = crate::http::request::percent_decode(k);
                    let val = UntrustedString::new(crate::http::request::percent_decode(v));
                    query_params.entry(key).or_default().push(val);
                }
            }
            crate::http::request::percent_decode(base_path)
        } else {
            crate::http::request::percent_decode(full_path)
        };

        // Path Canonicalization (1D)
        let path = sanitize_path(&raw_path);
        let uri = path.clone();

        // Multi-value Headers (1B)
        let mut headers: HashMap<String, Vec<String>> = HashMap::new();
        for line in lines {
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_lowercase();
                let val = v.trim().to_string();
                headers.entry(key).or_default().push(val);
            }
        }

        Ok(Request {
            method,
            path,
            uri,
            headers,
            body: body_bytes,
            query: query_params,
        })
    }

    pub fn parse_form_body(&self) -> FormData {
        let mut form_data = FormData::new();

        let content_type = match self.headers.get("content-type").and_then(|v| v.first()) {
            Some(ct) => ct,
            None => return form_data,
        };

        if content_type.starts_with("application/x-www-form-urlencoded") {
            if let Ok(body_str) = std::str::from_utf8(&self.body) {
                for pair in body_str.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        let key = crate::http::request::percent_decode(k);
                        let val = UntrustedString::new(crate::http::request::percent_decode(v));
                        form_data.fields.entry(key).or_default().push(val);
                    }
                }
            }
        } else if content_type.starts_with("multipart/form-data") {
            if let Some(boundary_idx) = content_type.find("boundary=") {
                let raw_boundary = content_type[boundary_idx + 9..]
                    .split(';')
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_matches('"');

                let boundary = format!("--{}", raw_boundary);
                let boundary_bytes = boundary.as_bytes();

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
                    if part.is_empty() || part == b"\r\n" || part == b"--\r\n" {
                        continue;
                    }

                    if let Some(header_end) = part.windows(4).position(|w| w == b"\r\n\r\n") {
                        let header_part = &part[..header_end];
                        let data_part = &part[header_end + 4..];

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
                                    if let Some(n_idx) = line.find("name=") {
                                        let rem = line[n_idx + 5..].trim_matches(';');
                                        name = rem.trim_matches('"').split(';').next().unwrap_or("").to_string();
                                    }

                                    // Robust Filename Extraction (Unquoted + RFC 2231) (2C)
                                    if let Some(f_idx) = line.find("filename*=") {
                                        let rem = &line[f_idx + 10..];
                                        if let Some(val) = rem.split("''").nth(1) {
                                            filename = Some(crate::http::request::percent_decode(val.trim_matches('"')));
                                        }
                                    } else if let Some(f_idx) = line.find("filename=") {
                                        let rem = &line[f_idx + 9..];
                                        let raw_f = rem.split(';').next().unwrap_or("").trim().trim_matches('"');
                                        filename = Some(raw_f.to_string());
                                    }
                                } else if line.to_lowercase().starts_with("content-type:") {
                                    part_content_type = line[13..].trim().to_string();
                                }
                            }

                            if !name.is_empty() {
                                if let Some(fname) = filename {
                                    form_data.files.entry(name).or_default().push(UploadedFile {
                                        filename: fname,
                                        content_type: part_content_type,
                                        data: final_data.to_vec(),
                                    });
                                } else if let Ok(val_str) = std::str::from_utf8(final_data) {
                                    form_data.fields.entry(name).or_default().push(
                                        UntrustedString::new(val_str.to_string())
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        form_data
    }

    pub fn parse_json_body<T: serde::de::DeserializeOwned>(&self) -> Result<T, String> {
        let content_type = self
            .headers
            .get("content-type")
            .and_then(|vals| vals.first())
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

    pub fn has_header(&self, key: &str) -> bool {
        self.headers.contains_key(&key.to_lowercase())
    }
}