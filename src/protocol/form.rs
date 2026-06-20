use crate::security::xss::{SafeHtml, Sanitizer, UntrustedString};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct UploadedFile {
    pub filename: String,
    pub content_type: String,
    pub data: Vec<u8>, // The raw binary contents of the file
}

#[derive(Debug, Clone)]
pub struct FormData {
    // Standard form inputs: e.g., name="username", value="admin"
    pub fields: HashMap<String, UntrustedString>,
    // Binary file uploads: e.g., name="avatar", value=UploadedFile
    pub files: HashMap<String, UploadedFile>,
}

impl FormData {
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
            files: HashMap::new(),
        }
    }

    /// Safely encodes a text field into an HTML-escaped `SafeHtml` type wrapper.
    /// This is perfect for direct injection into templates/Maud views.
    pub fn get_safe_html(&self, key: &str) -> Option<SafeHtml> {
        self.fields
            .get(key)
            .cloned() // Clone the UntrustedString to pass ownership to the encoder
            .map(|untrusted| Sanitizer::encode(untrusted))
    }

    /// Safely extracts the plain text value as an unescaped standard `String`.
    /// Use this for internal values
    pub fn get_plain_str(&self, key: &str) -> Option<&str> {
        self.fields.get(key).map(|untrusted| untrusted.as_str())
    }

    /// Gets a text field and attempts to parse it into an expected primitive type
    /// (like i32, u64, bool) directly from its underlying string slice.
    pub fn get_parsed<T>(&self, key: &str) -> Option<T>
    where
        T: std::str::FromStr,
    {
        let raw_str = self.get_plain_str(key)?;
        raw_str.parse::<T>().ok()
    }

    /// Retrieves an uploaded binary file asset from the form data map.
    pub fn get_file(&self, key: &str) -> Option<&UploadedFile> {
        self.files.get(key)
    }
}
