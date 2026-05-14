use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use crate::protocol::form::{FormData, UploadedFile};
use crate::security::xss::UntrustedString;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    PATCH,
    DELETE,
    UNKNOWN,
}

pub struct Request {
    pub method: HttpMethod,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
    pub query: HashMap<String, UntrustedString>,
}

impl Request {
    pub fn parse(stream: &TcpStream) -> Result<Self, String> {
        let start_time = Instant::now();
        let global_timeout = Duration::from_secs(10);

        let mut reader = BufReader::new(stream);

        if start_time.elapsed() > global_timeout {
            return Err("Total request time exceeded limit".to_string());
        }

        // "GET /index.html HTTP/1.1"
        let mut first_line = String::new();

        if let Err(e) = reader.read_line(&mut first_line) {
            if e.kind() == std::io::ErrorKind::WouldBlock
                || e.kind() == std::io::ErrorKind::TimedOut
            {
                return Err(format!("{}", "Connection timed out".to_string()));
            }
            return Err(format!("I/O Error: {}", e));
        }

        let parts: Vec<&str> = first_line.split_whitespace().collect();

        if parts.len() < 3 {
            return Err("Malformed request line".to_string());
        }

        let method = match parts[0] {
            "GET" => HttpMethod::GET,
            "POST" => HttpMethod::POST,
            "PUT" => HttpMethod::PUT,
            "PATCH" => HttpMethod::PATCH,
            "DELETE" => HttpMethod::DELETE,
            _ => HttpMethod::UNKNOWN,
        };

        let full_path = parts[1].to_string();

        let mut query_params = HashMap::new();

        // SPLIT PATH AND QUERY
        let path = if let Some((base_path, query_str)) = full_path.split_once('?') {
            // Parse query string: k1=v1&k2=v2
            for pair in query_str.split('&') {
                if let Some((k, v)) = pair.split_once('=') {
                    query_params.insert(k.to_string(), UntrustedString::new(v.to_string()));
                }
            }
            base_path.to_string()
        } else {
            full_path
        };

        let mut headers: HashMap<String, String> = HashMap::new();

        loop {
            let mut line = String::new();
            reader
                .read_line(&mut line)
                .map_err(|_| "Error reading header")?;

            if line == "\r\n" || line == "\n" || line.is_empty() {
                break;
            }

            if let Some((k, v)) = line.split_once(":") {
                headers.insert(k.trim().to_lowercase(), v.trim().to_string());
            }
        }

        // Security Check: Content-Length Limit
        let mut body = Vec::new();
        if let Some(str_len) = headers.get("content-length") {
            let len: usize = str_len
                .trim()
                .parse()
                .map_err(|_| "Invalid Content-Length")?;

            if len > 1024 * 1024 {
                // 1MB Limit
                return Err("Request body too large".to_string());
            }

            body.resize(len, 0);

            reader
                .read_exact(&mut body)
                .map_err(|_| "Failed to read body")?;

            if start_time.elapsed() > global_timeout {
                return Err("Timeout: Total request time exceeded".into());
            }
        }

        Ok(Request {
            method,
            path,
            headers,
            body,
            query: query_params,
        })
    }

    pub fn parse_form_body(&self) -> FormData {
        let mut form_data = FormData::new();
        let content_type = match self.headers.get("content-type") {
            Some(ct) => ct,
            None => return form_data,
        };

        // Case A: Standard application/x-www-form-urlencoded (No files, text only)
        if content_type.starts_with("application/x-www-form-urlencoded") {
            if let Ok(body_str) = std::str::from_utf8(&self.body) {
                for pair in body_str.split('&') {
                    if let Some((k, v)) = pair.split_once('=') {
                        // In a production engine, you'd apply URL decoding here
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
}
