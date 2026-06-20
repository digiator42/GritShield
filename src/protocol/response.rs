use std::collections::HashMap;

use crate::{
    security::xss::{SafeHtml, Sanitizer},
    utils::fs,
};

#[derive(Debug, Clone)]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

#[derive(Debug, Clone)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub max_age: u64,    // In seconds
    pub http_only: bool, // Prevents JS access
    pub secure: bool,    // Only sent over HTTPS
    pub same_site: SameSite,
}

impl Cookie {
    pub fn new(name: &str, value: &str) -> Self {
        Cookie {
            name: name.to_string(),
            value: value.to_string(),
            max_age: 3600,               // Default to 1 hour
            http_only: true,             // Default to True
            secure: true,                // Default to True
            same_site: SameSite::Strict, // Default to Strict
        }
    }

    /// Allows disabling the HTTPS requirement for local development testing
    pub fn set_secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    /// Allows changing SameSite restrictions (e.g., Lax for easier local redirection testing)
    pub fn set_same_site(mut self, same_site: SameSite) -> Self {
        self.same_site = same_site;
        self
    }
}

pub enum ResponseBody {
    Html(SafeHtml),
    StaticFile(String),
    Json(String),
}

/// Framework extension trait to safely convert multiple variants into structural response bodies
pub trait IntoResponseBody {
    fn convert(self) -> (ResponseBody, String); // Returns (Body variant, Default Content-Type)
}

// 1. Support Safe HTML (Maud markup/sanitizer objects)
impl IntoResponseBody for SafeHtml {
    fn convert(self) -> (ResponseBody, String) {
        (
            ResponseBody::Html(self),
            "text/html; charset=utf-8".to_string(),
        )
    }
}

// 2. Support Raw Strings or Formatted Message text
impl IntoResponseBody for String {
    fn convert(self) -> (ResponseBody, String) {
        // We assume raw strings are meant to be sent as safe plaintext/html bodies
        (
            ResponseBody::Html(Sanitizer::trust(&self)),
            "text/html; charset=utf-8".to_string(),
        )
    }
}

impl IntoResponseBody for &'static str {
    fn convert(self) -> (ResponseBody, String) {
        (
            ResponseBody::Html(Sanitizer::trust(self)),
            "text/html; charset=utf-8".to_string(),
        )
    }
}

// 3. Create a wrapper struct specifically for explicit JSON data structures
pub struct JsonPayload<T>(pub T);

impl<T: serde::Serialize> IntoResponseBody for JsonPayload<T> {
    fn convert(self) -> (ResponseBody, String) {
        let json_string = serde_json::to_string(&self.0)
            .unwrap_or_else(|_| r#"{"error": "Internal Server Serialization Error"}"#.to_string());
        (
            ResponseBody::Json(json_string),
            "application/json; charset=utf-8".to_string(),
        )
    }
}

// 1. Support owned HashMaps, e.g., HashMap<K, V>
impl<K, V> IntoResponseBody for HashMap<K, V>
where
    K: serde::Serialize + std::hash::Hash + Eq,
    V: serde::Serialize,
{
    fn convert(self) -> (ResponseBody, String) {
        let json_string = serde_json::to_string(&self)
            .unwrap_or_else(|_| r#"{"error": "Internal Server Serialization Error"}"#.to_string());
        (
            ResponseBody::Json(json_string),
            "application/json; charset=utf-8".to_string(),
        )
    }
}

// 2. Support borrowed HashMaps, e.g., &HashMap<K, V>
impl<K, V> IntoResponseBody for &HashMap<K, V>
where
    K: serde::Serialize + std::hash::Hash + Eq,
    V: serde::Serialize,
{
    fn convert(self) -> (ResponseBody, String) {
        let json_string = serde_json::to_string(self)
            .unwrap_or_else(|_| r#"{"error": "Internal Server Serialization Error"}"#.to_string());
        (
            ResponseBody::Json(json_string),
            "application/json; charset=utf-8".to_string(),
        )
    }
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub cookies: Vec<Cookie>,
    pub body: ResponseBody,
}

impl Response {
    pub fn new(status: u16, body: SafeHtml) -> Self {
        Response {
            status,
            headers: vec![
                (
                    "Content-Type".to_string(),
                    "text/html; charset=utf-8".to_string(),
                ),
                ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
                ("X-Frame-Options".to_string(), "DENY".to_string()),
            ],
            cookies: Vec::new(),
            body: ResponseBody::Html(body),
        }
    }

    /// Luxury modifier to attach a dynamic cookie wrapper directly to the response state
    pub fn with_cookie(mut self, cookie: Cookie) -> Self {
        self.cookies.push(cookie);
        self
    }

    pub fn static_file(path: &str) -> Self {
        Response {
            status: 200,
            headers: vec![
                ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
                ("X-Frame-Options".to_string(), "DENY".to_string()),
            ],
            cookies: Vec::new(),
            body: ResponseBody::StaticFile(path.to_string()),
        }
    }

    /// Serializes the response into raw bytes for the TCP stream safely
    pub fn to_bytes(&self, body_bytes: &[u8], content_type: &str) -> Vec<u8> {
        let mut response = format!("HTTP/1.1 {} OK\r\n", self.status);

        response.push_str(&format!("Content-Type: {}\r\n", content_type));

        // Write the real byte count directly from the byte slice buffer!
        response.push_str(&format!("Content-Length: {}\r\n", body_bytes.len()));

        // Add standard headers, skipping keys we've already written explicitly (case-insensitive)
        for (key, value) in &self.headers {
            let normalized_key = key.to_lowercase();

            // Skip duplicates to prevent splitting the browser frame pipeline
            if normalized_key == "content-type" || normalized_key == "content-length" {
                continue;
            }

            response.push_str(&format!("{}: {}\r\n", key, value));
        }

        for cookie in &self.cookies {
            let same_site_str = match cookie.same_site {
                SameSite::Strict => "Strict",
                SameSite::Lax => "Lax",
                SameSite::None => "None",
            };

            let mut cookie_str = format!(
                "Set-Cookie: {}={}; Max-Age={}; SameSite={}; Path=/",
                cookie.name, cookie.value, cookie.max_age, same_site_str
            );

            if cookie.http_only {
                cookie_str.push_str("; HttpOnly");
            }
            if cookie.secure {
                cookie_str.push_str("; Secure");
            }

            response.push_str(&format!("{}\r\n", cookie_str));
        }

        // Terminate header parsing sequence cleanly
        response.push_str("\r\n");

        let mut raw = response.into_bytes();
        raw.extend_from_slice(body_bytes);

        raw
    }

    /// A premium API helper that serializes data structure payloads automatically
    pub fn json<T: serde::Serialize>(status: u16, data: &T) -> Self {
        let json_string = serde_json::to_string(data)
            .unwrap_or_else(|_| r#"{"error": "Internal Server Serialization Error"}"#.to_string());

        Response {
            status,
            headers: vec![
                (
                    "Content-Type".to_string(),
                    "application/json; charset=utf-8".to_string(),
                ),
                ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
            ],
            cookies: Vec::new(),
            body: ResponseBody::Json(json_string),
        }
    }

    /// Kernel Resolver modification to emit bytes
    pub fn resolve(&self) -> (Vec<u8>, String) {
        match &self.body {
            ResponseBody::Html(html) => (html.as_bytes().to_vec(), "text/html".to_string()),
            ResponseBody::Json(json_str) => {
                (json_str.as_bytes().to_vec(), "application/json".to_string())
            }
            ResponseBody::StaticFile(path) => fs::serve_static(path).unwrap_or_else(|_| {
                (
                    Sanitizer::trust("<h1>404 File Not Found</h1>")
                        .as_bytes()
                        .to_vec(),
                    "text/html".to_string(),
                )
            }),
        }
    }

    /// Creates an HTTP redirect response (typically 302 Found or 303 See Other)
    /// forcing the browser to seamlessly navigate to a target destination URL.
    pub fn redirect(status: u16, location: &str) -> Self {
        Response {
            status,
            headers: vec![
                ("Location".to_string(), location.to_string()),
                (
                    "Content-Type".to_string(),
                    "text/html; charset=utf-8".to_string(),
                ),
                ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
                ("X-Frame-Options".to_string(), "DENY".to_string()),
            ],
            cookies: Vec::new(),
            body: ResponseBody::Html(Sanitizer::trust(
                format!(
                    "Redirecting to <a href=\"{}\">{}</a>...",
                    location, location
                )
                .as_str(),
            )),
        }
    }

    /// Return a proper HTTP 302 response with the right headers and a simple HTML body
    /// forcing the browser to seamlessly navigate to a target destination URL.
    pub fn navigate_to(location: &str) -> Response {
        Response {
            status: 302, // standard redirect status
            headers: vec![
                ("Location".to_string(), location.to_string()),
                (
                    "Content-Type".to_string(),
                    "text/html; charset=utf-8".to_string(),
                ),
                ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
                ("X-Frame-Options".to_string(), "DENY".to_string()),
            ],
            cookies: Vec::new(),
            body: ResponseBody::Html(Sanitizer::trust(
                format!(
                    "Redirecting to <a href=\"{}\">{}</a>...",
                    location, location
                )
                .as_str(),
            )),
        }
    }

    /// Core polymorphic base constructor utilizing the IntoResponseBody converter pipeline
    pub fn build<B: IntoResponseBody>(status: u16, payload: B) -> Self {
        let (body, content_type) = payload.convert();

        Response {
            status,
            headers: vec![
                ("Content-Type".to_string(), content_type),
                ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
                ("X-Frame-Options".to_string(), "DENY".to_string()),
            ],
            cookies: Vec::new(),
            body,
        }
    }

    // --- 2xx SUCCESS RESPONSES ---

    /// 200 OK — Standard success response
    pub fn ok<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(200, payload)
    }

    /// 201 Created — Resource successfully created
    pub fn created<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(201, payload)
    }

    // --- 4xx CLIENT ERRORS ---

    /// 400 Bad Request — Malformed syntax or missing validation constraints
    pub fn bad_request<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(400, payload)
    }

    /// 401 Unauthorized — Authentication is missing or invalid
    pub fn unauthorized<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(401, payload)
    }

    /// 403 Forbidden — lacks permissions
    pub fn forbidden<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(403, payload)
    }

    /// 404 Not Found — Resource or path cannot be resolved
    pub fn not_found<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(404, payload)
    }

    /// 409 Conflict — Request conflict with the current state of the target resource.
    pub fn conflict<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(409, payload)
    }

    /// 429 Too Many Requests — client has sent too many requests in a given amount of time
    pub fn too_many_requests<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(429, payload)
    }

    // --- 5xx SERVER ERRORS ---

    /// 500 Internal Server Error — Generic catch-all for database faults or crypto crashes
    pub fn internal_error<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(500, payload)
    }
}
