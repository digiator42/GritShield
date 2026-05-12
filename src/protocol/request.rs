use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

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
        reader.read_line(&mut first_line).map_err(|e| {
            println!("{}", e);
            "Failed to read request line"
        })?;

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

        let path = parts[1].to_string();
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
        })
    }
}
