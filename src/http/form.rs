use serde::de::DeserializeOwned;

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
    pub fields: HashMap<String, Vec<UntrustedString>>,
    // Binary file uploads: e.g., name="avatar", value=UploadedFile
    pub files: HashMap<String, Vec<UploadedFile>>,
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
            .and_then(|values| values.first()
            .map(|untrusted| Sanitizer::encode(untrusted.as_str())))
    }

    /// Safely extracts the plain text value as an unescaped standard `String`.
    /// This is for internal values
    pub fn get_plain_str(&self, key: &str) -> Option<&str> {
        self.fields.get(key)?.first().map(|s| s.as_str())
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

   pub fn get_all_plain_str(&self, key: &str) -> Vec<&str> {
        self.fields
            .get(key)
            .map(|vec| vec.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default()
    }

    pub fn get_file(&self, key: &str) -> Option<&UploadedFile> {
        self.files.get(key)?.first()
    }

    pub fn get_all_files(&self, key: &str) -> Vec<&UploadedFile> {
        self.files
            .get(key)
            .map(|vec| vec.iter().collect())
            .unwrap_or_default()
    }

    /// Deserializes form fields into a strongly typed struct using `serde_json`
    pub fn populate<T: DeserializeOwned>(&self) -> Result<T, String> {
        let mut map = serde_json::Map::new();
        for (k, v) in &self.fields {
            if v.len() == 1 {
                map.insert(k.clone(), serde_json::Value::String(v[0].as_str().to_string()));
            } else {
                let arr = v
                    .iter()
                    .map(|s| serde_json::Value::String(s.as_str().to_string()))
                    .collect();
                map.insert(k.clone(), serde_json::Value::Array(arr));
            }
        }
        serde_json::from_value(serde_json::Value::Object(map))
            .map_err(|e| format!("Form Deserialization Error: {}", e))
    }
}
