use std::fmt;

/// A wrapper around a String that has NOT been sanitized.
/// It cannot be printed or converted to a byte array directly.
#[derive(Debug, Clone)]
pub struct UntrustedString(String);

impl UntrustedString {
    /// Creates a new UntrustedString. Only the Kernel should do this
    /// during the Request Parsing phase.
    pub fn new(s: String) -> Self {
        UntrustedString(s)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The inner safe string for HTML rendering.
/// It can only be created by passing an UntrustedString through the sanitizer.
#[derive(Debug)]
pub struct SafeHtml(String);

impl SafeHtml {
    /// Allows the inner string to be converted to bytes for the raw TCP response.
    /// This is used internally by the Response Writer.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

// Implement Display for SafeHtml so it can be easily used in templates/format!
impl fmt::Display for SafeHtml {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct Sanitizer;

impl Sanitizer {
    /// The "Gatekeeper" function.
    pub fn encode(untrusted: UntrustedString) -> SafeHtml {
        // Rust's functional approach makes this clear
        let encoded: String = untrusted
            .0
            .chars()
            .map(|c| match c {
                '&' => "&amp;".to_string(),
                '<' => "&lt;".to_string(),
                '>' => "&gt;".to_string(),
                '"' => "&quot;".to_string(),
                '\'' => "&#x27;".to_string(),
                '/' => "&#x2F;".to_string(),
                c => c.to_string(),
            })
            .collect();

        // We now "wrap" the encoded string in the SafeHtml "promise"
        SafeHtml(encoded)
    }

    /// Allow developers to trust hardcoded strings
    /// e.g., Sanitizer::trust("<h1>Welcome</h1>")
    pub fn trust(safe_str: &str) -> SafeHtml {
        SafeHtml(safe_str.to_string())
    }
}
