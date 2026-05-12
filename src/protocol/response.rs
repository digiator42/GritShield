use crate::security::xss::SafeHtml;

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

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub cookies: Vec<Cookie>,
    pub body: SafeHtml,
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
            body,
        }
    }

    /// Serializes the response into raw bytes for the TCP stream
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut response = format!("HTTP/1.1 {} OK\r\n", self.status);

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
        raw.extend_from_slice(self.body.as_bytes());
        
        raw
    }
}
