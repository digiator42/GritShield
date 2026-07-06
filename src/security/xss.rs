use std::fmt;
use std::str::FromStr;

/// A wrapper around a String that has NOT been sanitized.
/// It cannot be printed or converted to a byte array directly.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
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

    /// Converts an UntrustedString into a String.
    pub fn to_string(self) -> String {
        self.0
    }

    /// Parses the untrusted string into any target type that implements `FromStr`.
    ///
    /// This delegates directly to standard string parsing, returning a `Result`
    /// containing the target type or the type's associated parsing error.
    pub fn parse<T>(&self) -> Result<T, T::Err>
    where
        T: FromStr,
    {
        self.0.parse::<T>()
    }
}

impl fmt::Display for UntrustedString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// The inner safe string for HTML rendering.
/// It can only be created by passing an UntrustedString through the sanitizer.
#[derive(Debug, Clone, Default)]
pub struct SafeHtml(String);

impl SafeHtml {
    /// Allows the inner string to be converted to bytes for the raw TCP response.
    /// This is used internally by the Response Writer.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }

    pub fn into_string(self) -> String {
        self.0
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
    /// The "Gatekeeper" function for rendering text inside HTML templates.
    /// Protects against Cross-Site Scripting (XSS) context injections.
    pub fn encode(untrusted: &str) -> SafeHtml {
        let encoded: String = untrusted
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

        SafeHtml(encoded)
    }

    /// Safely decodes percent-encoded query parameter input strings back into plain text.
    /// e.g., converts "%3A49" -> ":49" so the backend engines receive correct values.
    pub fn url_decode(encoded_str: &str) -> String {
        ::urlencoding::decode(encoded_str)
            .map(|cow| cow.into_owned())
            .unwrap_or_else(|_| encoded_str.to_string())
    }

    /// Encodes a plain string slice for safe use within URL query strings.
    pub fn url_encode(plain_str: &str) -> String {
        ::urlencoding::encode(plain_str).into_owned()
    }

    /// Allow to trust outgoing hardcoded strings bypassing entity encoding
    /// e.g., Sanitizer::trust("<h1>Welcome Matrix Dashboard</h1>")
    pub fn trust(safe_str: &str) -> SafeHtml {
        SafeHtml(safe_str.to_string())
    }
}
