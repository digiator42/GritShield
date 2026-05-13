use crate::{security::xss::{SafeHtml, Sanitizer}, utils::fs};

pub enum SameSite {
    Strict,
    Lax,
    None,
}

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
}

pub enum ResponseBody {
    Html(SafeHtml),
    StaticFile(String),
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
                ("Content-Type".to_string(), "text/html".to_string()),
                ("X-Content-Type-Options".to_string(), "nosniff".to_string()),
                ("X-Frame-Options".to_string(), "DENY".to_string()),
            ],
            cookies: Vec::new(),
            body: ResponseBody::Html(body),
        }
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

    /// Serializes the response into raw bytes for the TCP stream
    pub fn to_bytes(&self, body_bytes: &[u8], content_type: &str) -> Vec<u8> {
        let mut response = format!("HTTP/1.1 {} OK\r\n", self.status);

        response.push_str(&format!("Content-Type: {}\r\n", content_type));

        // Add standard headers
        for (key, value) in &self.headers {
            response.push_str(&format!("{}: {}\r\n", key, value));
        }

        for cookie in &self.cookies {
            let same_site_str = match cookie.same_site {
                SameSite::Strict => "Strict",
                SameSite::Lax => "Lax",
                SameSite::None => "None",
            };

            let mut cookie_str = format!(
                "Set-Cookie: {}={}; Max-Age={}; SameSite={}",
                cookie.name, cookie.value, cookie.max_age, same_site_str
            );

            if cookie.http_only {
                cookie_str.push_str("; HttpOnly");
            }
            if cookie.secure {
                cookie_str.push_str("; Secure");
            }

            println!("{}", cookie_str);

            response.push_str(&format!("{}\r\n", cookie_str));
        }

        response.push_str("\r\n");
        let mut raw = response.into_bytes();
        raw.extend_from_slice(body_bytes);

        raw
    }

    /// The Kernel Resolver: Converts the abstract body into raw bytes and a MIME type
    pub fn resolve(&self) -> (Vec<u8>, String) {
        match &self.body {
            ResponseBody::Html(html) => (html.as_bytes().to_vec(), "text/html".to_string()),
            ResponseBody::StaticFile(path) => {
                // Use our secure fs utility to fetch file data
                fs::serve_static(path).unwrap_or_else(|_| {
                    (
                        Sanitizer::trust("<h1>404 File Not Found</h1>")
                            .as_bytes()
                            .to_vec(),
                        "text/html".to_string(),
                    )
                })
            }
        }
    }
}
