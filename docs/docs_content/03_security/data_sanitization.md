# Data Sanitization 

GritShield provides built-in, compile-time payload sanitization macros. It runs immediately after deserialization, cleansing input fields (trimming whitespace, escaping XSS vectors, normalizing cases) before your controller handlers ever see the data.

## Quick Start

Derive `GritSanitizer` on your request DTO and annotate string fields with `#[clean(...)]`.

Rust

```rust
use gritshield::GritSanitizer;
use serde::Deserialize;

#[derive(Deserialize, GritSanitizer)]
pub struct CreateUserPayload {
    // Trims leading/trailing whitespace and converts to lowercase
    #[clean(trim, lowercase)]
    pub email: String,

    // Trims whitespace and encodes HTML/XSS vectors safely
    #[clean(trim, html_escape)]
    pub bio: String,

    // Non-string or untreated fields pass through untouched
    pub age: u8,
}
```

## Available Field Attributes (`#[clean(...)]`)

Apply these options inside `#[clean(...)]` to customize field processing:

|**Attribute**|**Behavior**|**Example Input**|**Result**|
|---|---|---|---|
|`trim`|Strips leading/trailing whitespace|`" alice "`|`"alice"`|
|`html_escape`|Encodes HTML special characters to prevent XSS|`"<script>"`|`"&lt;script&gt;"`|
|`lowercase`|Converts string to lowercase|`"USER@ExAmPlE.com"`|`"user@example.com"`|
|`uppercase`|Converts string to uppercase|`"us"`|`"US"`|
|`url_decode`|Decodes percent-encoded URL strings|`"hello%20world"`|`"hello world"`|
|`nested`|Recursively invokes `.sanitize()` on nested DTOs|`Address` / `Vec<T>` / `Option<T>`|Cleans inner fields|