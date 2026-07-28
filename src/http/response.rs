use std::collections::HashMap;

use crate::{
    security::xss::{SafeHtml, Sanitizer},
    utils::fs,
};
use serde::Serialize;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum HttpStatus {
    // --- 2xx Success ---
    Ok = 200,
    Created = 201,
    Accepted = 202,
    NoContent = 204,

    // --- 3xx Redirection ---
    MovedPermanently = 301,
    Found = 302,
    SeeOther = 303,
    NotModified = 304,

    // --- 4xx Client Errors ---
    BadRequest = 400,
    Unauthorized = 401,
    Forbidden = 403,
    NotFound = 404,
    Conflict = 409,
    UnprocessableEntity = 422,
    TooManyRequests = 429,

    // --- 5xx Server Errors ---
    InternalServerError = 500,
    NotImplemented = 501,
    BadGateway = 502,
    ServiceUnavailable = 503,
}

impl HttpStatus {
    pub fn code(self) -> u16 {
        self as u16
    }
}

#[derive(Clone)]
pub enum ResponseBody {
    Html(SafeHtml),
    StaticFile(String),
    Json(String),
}

// impl as_str to ResponseBody
impl ResponseBody {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ResponseBody::Html(safe_html) => std::str::from_utf8(safe_html.as_bytes()).ok(),
            ResponseBody::StaticFile(_) => None,
            ResponseBody::Json(json_str) => Some(json_str.as_str()),
        }
    }
}

/// Framework extension trait to safely convert multiple variants into structural response bodies
pub trait IntoResponseBody {
    fn convert(self) -> (ResponseBody, String); // Returns (Body variant, Default Content-Type)
}

// Support Safe HTML (Maud markup/sanitizer objects)
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

// Create a wrapper struct specifically for explicit JSON data structures
#[derive(Serialize)]
pub struct JsonPayload<T>(pub T);

#[derive(Serialize)]
pub struct HtmlPayload<T>(pub T);

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

// Support owned HashMaps, e.g., HashMap<K, V>
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

// Support borrowed HashMaps, e.g., &HashMap<K, V>
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

// Support for Maud Templates Integration
impl IntoResponseBody for maud::PreEscaped<String> {
    fn convert(self) -> (ResponseBody, String) {
        // Maud guarantees its compiled payload buffer is already clean of XSS vulnerabilities.
        // We unpack the inner string out of the tuple struct wrapper safely.
        let compiled_html = self.0;

        (
            ResponseBody::Html(Sanitizer::trust(&compiled_html)),
            "text/html; charset=utf-8".to_string(),
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
        use std::io::Write;

        // Write straight into the final buffer instead of building an intermediate
        // `String` through a chain of `format!`/`push_str` calls — each `format!`
        // was its own heap allocation, on top of `response` itself reallocating as
        // it grew past its starting capacity.
        let mut raw = Vec::with_capacity(256 + body_bytes.len());

        let _ = write!(raw, "HTTP/1.1 {} OK\r\n", self.status);
        let _ = write!(raw, "Content-Type: {}\r\n", content_type);
        let _ = write!(raw, "Content-Length: {}\r\n", body_bytes.len());

        // Add standard headers, skipping keys we've already written explicitly (case-insensitive)
        for (key, value) in &self.headers {
            // Skip duplicates to prevent splitting the browser frame pipeline
            if key.eq_ignore_ascii_case("content-type")
                || key.eq_ignore_ascii_case("content-length")
            {
                continue;
            }

            let _ = write!(raw, "{}: {}\r\n", key, value);
        }

        for cookie in &self.cookies {
            let same_site_str = match cookie.same_site {
                SameSite::Strict => "Strict",
                SameSite::Lax => "Lax",
                SameSite::None => "None",
            };

            let _ = write!(
                raw,
                "Set-Cookie: {}={}; Max-Age={}; SameSite={}; Path=/",
                cookie.name, cookie.value, cookie.max_age, same_site_str
            );

            if cookie.http_only {
                let _ = raw.write_all(b"; HttpOnly");
            }
            if cookie.secure {
                let _ = raw.write_all(b"; Secure");
            }

            let _ = raw.write_all(b"\r\n");
        }

        // Terminate header parsing sequence cleanly
        let _ = raw.write_all(b"\r\n");
        raw.extend_from_slice(body_bytes);

        raw
    }

    /// A premium API helper that serializes data structure payloads automatically
    pub fn json<T: serde::Serialize>(status: HttpStatus, data: &T) -> Self {
        let json_string = serde_json::to_string(data)
            .unwrap_or_else(|_| r#"{"error": "Internal Server Serialization Error"}"#.to_string());

        Response {
            status: status.code(),
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

    /// Attach a custom header dynamically to the response (Builder pattern)
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }

    /// Attach multiple custom headers at once
    pub fn with_headers(mut self, headers: Vec<(String, String)>) -> Self {
        self.headers.extend(headers);
        self
    }

    /// Build a JSON response with a specific status code and data (returns the response)
    pub fn json_with<T: serde::Serialize>(status: HttpStatus, data: &T) -> Self {
        Self::json(status, data)
    }

    // ─── 2xx JSON SUCCESS RESPONSES ───

    /// 200 OK — JSON success
    pub fn json_ok<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::Ok, data)
    }

    /// 201 Created — JSON success
    pub fn json_created<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::Created, data)
    }

    /// 202 Accepted — JSON success
    pub fn json_accepted<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::Accepted, data)
    }

    /// 204 No Content — Empty JSON response (no body)
    pub fn json_no_content() -> Self {
        Self::json(HttpStatus::NoContent, &serde_json::json!({}))
    }

    // ─── 4xx JSON ERROR RESPONSES ───

    /// 400 Bad Request — JSON error
    pub fn json_bad_request<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::BadRequest, data)
    }

    /// 401 Unauthorized — JSON error
    pub fn json_unauthorized<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::Unauthorized, data)
    }

    /// 403 Forbidden — JSON error
    pub fn json_forbidden<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::Forbidden, data)
    }

    /// 404 Not Found — JSON error
    pub fn json_not_found<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::NotFound, data)
    }

    /// 409 Conflict — JSON error
    pub fn json_conflict<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::Conflict, data)
    }

    /// 422 Unprocessable Entity — JSON validation error
    pub fn json_unprocessable<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::UnprocessableEntity, data)
    }

    /// 429 Too Many Requests — JSON error
    pub fn json_too_many_requests<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::TooManyRequests, data)
    }

    // ─── 5xx JSON ERROR RESPONSES ───

    /// 500 Internal Server Error — JSON error
    pub fn json_internal_error<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::InternalServerError, data)
    }

    /// 501 Not Implemented — JSON error
    pub fn json_not_implemented<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::NotImplemented, data)
    }

    /// 503 Service Unavailable — JSON error
    pub fn json_service_unavailable<T: serde::Serialize>(data: &T) -> Self {
        Self::json(HttpStatus::ServiceUnavailable, data)
    }

    // ─── CONVENIENCE: Quick JSON Errors (Common Patterns) ───

    /// 400 Bad Request — Quick error message
    pub fn json_error(message: impl Into<String>) -> Self {
        Self::json_bad_request(&serde_json::json!({
            "error": message.into()
        }))
    }

    /// 404 Not Found — Quick not found response
    pub fn json_not_found_msg(message: impl Into<String>) -> Self {
        Self::json_not_found(&serde_json::json!({
            "error": message.into()
        }))
    }

    /// 401 Unauthorized — Quick unauthorized response
    pub fn json_unauthorized_msg(message: impl Into<String>) -> Self {
        Self::json_unauthorized(&serde_json::json!({
            "error": message.into()
        }))
    }

    /// 403 Forbidden — Quick forbidden response
    pub fn json_forbidden_msg(message: impl Into<String>) -> Self {
        Self::json_forbidden(&serde_json::json!({
            "error": message.into()
        }))
    }

    /// 422 Validation Error — Quick validation error
    pub fn json_validation_error(errors: HashMap<String, Vec<String>>) -> Self {
        Self::json_unprocessable(&serde_json::json!({
            "errors": errors
        }))
    }

    /// 500 Internal Error — Quick internal error
    pub fn json_internal_error_msg(message: impl Into<String>) -> Self {
        Self::json_internal_error(&serde_json::json!({
            "error": message.into()
        }))
    }

    // ─── Core polymorphic base constructor ───
    pub fn build<B: IntoResponseBody>(status: HttpStatus, payload: B) -> Self {
        let (body, content_type) = payload.convert();

        Response {
            status: status.code(),
            headers: vec![
                ("Content-Type".to_string(), content_type),
                ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
                ("X-Frame-Options".to_string(), "DENY".to_string()),
            ],
            cookies: Vec::new(),
            body,
        }
    }

    // --- 2xx SUCCESS RESPONSES (HTML/General) ---
    pub fn ok<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(HttpStatus::Ok, payload)
    }

    pub fn created<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(HttpStatus::Created, payload)
    }

    // --- 4xx CLIENT ERRORS (HTML/General) ---
    pub fn bad_request<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(HttpStatus::BadRequest, payload)
    }

    pub fn unauthorized<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(HttpStatus::Unauthorized, payload)
    }

    pub fn forbidden<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(HttpStatus::Forbidden, payload)
    }

    pub fn not_found<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(HttpStatus::NotFound, payload)
    }

    pub fn conflict<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(HttpStatus::Conflict, payload)
    }

    pub fn too_many_requests<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(HttpStatus::TooManyRequests, payload)
    }

    // --- 5xx SERVER ERRORS (HTML/General) ---
    pub fn internal_error<B: IntoResponseBody>(payload: B) -> Self {
        Self::build(HttpStatus::InternalServerError, payload)
    }
}
