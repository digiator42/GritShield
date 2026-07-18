# Generic Response System

GritShield provides a powerful and flexible `Response` builder that supports multiple payload types through clean, unified methods.

## Overview

The `Response` struct uses generic implementations, allowing you to return:
- Plain text (`&str`, `String`)
- Safe HTML (`SafeHtml`, Maud templates)
- JSON structures (via `JsonPayload` or directly with serializable types and `HashMap`)

All through convenient methods like `ok()`, `bad_request()`, `unauthorized()`, `forbidden()`, `created()`, etc.

## Usage Examples

### 1. Simple Text / String Responses

```rust
pub async fn handler(ctx: RequestContext) -> Response {
    if input_invalid {
        return Response::bad_request("The provided password does not meet security policies.");
    }
    Response::ok("Operation completed successfully.")
}
```

### 2. Safe HTML Responses

```rust
pub async fn handler(ctx: RequestContext) -> Response {
    let error_markup = Sanitizer::trust("<h1>Validation Error</h1><p>Invalid credentials</p>");
    
    Response::bad_request(error_markup)
}
```

Using Maud HTML:

```rust
Response::forbidden(html! {
    div class="error-box p-6 bg-red-950 text-red-200 rounded-lg" {
        h1 class="text-xl font-bold" { "🔒 Access Denied" }
        p { "You do not have permission to view this resource." }
    }
})
```

### 3. JSON Payloads with `JsonPayload`

```rust
#[derive(serde::Serialize)]
struct ErrorContainer {
    error: String,
    missing_fields: Vec<String>,
}

pub async fn handler(ctx: RequestContext) -> Response {
    let details = ErrorContainer {
        error: "Missing Form Values".to_string(),
        missing_fields: vec!["username".to_string()],
    };

    Response::bad_request(JsonPayload(details))
}
```

### 4. Direct HashMap → JSON

```rust
pub async fn handler(ctx: RequestContext) -> Response {
    // Pass by reference
    Response::bad_request(&HashMap::from([
        ("message", "This is a test endpoint"),
        ("status", "success"),
    ]))
}
```

```rust
pub async fn handler(ctx: RequestContext) -> Response {
    let mut error_map = HashMap::new();
    error_map.insert("error_code", 40012);
    error_map.insert("attempt_count", 3);

    Response::bad_request(error_map)
}
```

## Common Response Methods

| Method                        | Status | Use Case                        |
|-------------------------------|--------|---------------------------------|
| `Response::ok()`              | 200    | Success                         |
| `Response::created()`         | 201    | Resource successfully created   |
| `Response::bad_request()`     | 400    | Validation / client error       |
| `Response::unauthorized()`    | 401    | Authentication required         |
| `Response::forbidden()`       | 403    | Insufficient permissions        |
| `Response::not_found()`       | 404    | Resource not found              |
| `Response::redirect()`        | 303    | Redirect                        |
