use crate::security::xss::UntrustedString;
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
}
