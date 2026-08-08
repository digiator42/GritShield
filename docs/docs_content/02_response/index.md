# Generic Response System

GritShield provides a powerful and flexible `Response` builder that supports multiple payload types through clean, unified methods.

## Overview

The `Response` struct uses generic implementations, allowing you to return:
- Plain text (`&str`, `String`)
- Safe HTML (`SafeHtml`, Maud templates)
- JSON structures (via `JsonPayload`, `HtmlPayload`, or directly with serializable types and `HashMap`)
- Static files
- Custom headers and security cookies

All through convenient methods like `ok()`, `bad_request()`, `unauthorized()`, `forbidden()`, `created()`, etc.

## Cookie System

GritShield includes a secure cookie system with built-in security defaults:

```rust
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
```

### Cookie Examples

```rust
// Create a cookie with secure defaults (1 hour, HttpOnly, Secure, Strict)
let cookie = Cookie::new("session_id", "abc123");

// Customize for local development
let cookie = Cookie::new("session_id", "abc123")
    .set_secure(false)  // Disable HTTPS requirement for local testing
    .set_same_site(SameSite::Lax);  // Easier for local redirection

// Attach cookie to response
Response::ok("Login successful").with_cookie(cookie);
```

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

#### Raw HTML with Sanitizer

For raw HTML strings, use the `Sanitizer::trust()` method to safely wrap HTML content:

```rust
pub async fn handler(ctx: RequestContext) -> Response {
    let error_markup = Sanitizer::trust("<h1>Validation Error</h1><p>Invalid credentials</p>");
    
    Response::bad_request(error_markup)
}
```

#### Maud HTML Templates

Maud templates are automatically supported through the `IntoResponseBody` trait. Maud guarantees XSS-safe compiled HTML:

```rust
Response::forbidden(html! {
    div class="error-box p-6 bg-red-950 text-red-200 rounded-lg" {
        h1 class="text-xl font-bold" { "🔒 Access Denied" }
        p { "You do not have permission to view this resource." }
    }
})
```

#### Complex Maud Templates with Data

```rust
pub async fn user_profile(ctx: RequestContext) -> Response {
    let user = get_user(&ctx);
    
    Response::ok(html! {
        div class="profile-container" {
            h1 { (user.name) }
            p { (user.email) }
            div class="stats" {
                span { "Posts: " (user.post_count) }
                span { "Joined: " (user.join_date) }
            }
        }
    })
}
```

#### PreEscaped Maud Content

For content that should not be HTML-escaped, use `maud::PreEscaped`:

```rust
use maud::PreEscaped;

pub async fn rich_content(ctx: RequestContext) -> Response {
    let html_content = "<div><strong>Bold text</strong></div>";
    
    Response::ok(PreEscaped(html_content.to_string()))
}
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

### 4. HTML Payloads with `HtmlPayload`

```rust
#[derive(serde::Serialize)]
struct PageData {
    title: String,
    content: String,
}

pub async fn handler(ctx: RequestContext) -> Response {
    let page_data = PageData {
        title: "Welcome".to_string(),
        content: "Hello World".to_string(),
    };

    Response::ok(HtmlPayload(page_data))
}
```

### 5. Direct HashMap → JSON

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

### 6. Static File Serving

```rust
pub async fn handler(ctx: RequestContext) -> Response {
    Response::static_file("public/index.html")
}
```

### 7. Redirect Responses

```rust
// Custom redirect with specific status
pub async fn handler(ctx: RequestContext) -> Response {
    Response::redirect(303, "/dashboard")
}

// Simple navigation (302 Found)
pub async fn handler(ctx: RequestContext) -> Response {
    Response::navigate_to("/login")
}
```

### 8. Custom Headers

```rust
pub async fn handler(ctx: RequestContext) -> Response {
    Response::ok("Success")
        .with_header("X-Custom-Header", "custom-value")
        .with_header("Cache-Control", "no-cache")
}

// Multiple headers at once
pub async fn handler(ctx: RequestContext) -> Response {
    Response::ok("Success")
        .with_headers(vec![
            ("X-Custom-Header".to_string(), "value".to_string()),
            ("Cache-Control".to_string(), "no-cache".to_string()),
        ])
}
```

## Common Html Response Methods

| Method                        | Status | Use Case                        |
|-------------------------------|--------|---------------------------------|
| `Response::ok()`              | 200    | Success                         |
| `Response::created()`         | 201    | Resource successfully created   |
| `Response::bad_request()`     | 400    | Validation / client error       |
| `Response::unauthorized()`    | 401    | Authentication required         |
| `Response::forbidden()`       | 403    | Insufficient permissions        |
| `Response::not_found()`       | 404    | Resource not found              |
| `Response::conflict()`        | 409    | Resource conflict               |
| `Response::too_many_requests()`| 429   | Rate limiting                   |
| `Response::internal_error()`  | 500    | Server error                    |
| `Response::redirect()`        | 303/302| Redirect to URL                 |
| `Response::navigate_to()`     | 302    | Simple redirect                 |
| `Response::static_file()`     | 200    | Serve static files             |


## JSON-Specific Response Methods

For API endpoints, use the dedicated JSON response methods:

### JSON Success Responses

```rust
// 200 OK
Response::json_ok(&serde_json::json!({"status": "success"}))

// 201 Created
Response::json_created(&serde_json::json!({"id": 123}))

// 202 Accepted
Response::json_accepted(&serde_json::json!({"message": "Processing"}))

// 204 No Content
Response::json_no_content()
```

### JSON Error Responses

```rust
// 400 Bad Request
Response::json_bad_request(&serde_json::json!({"error": "Invalid input"}))

// 401 Unauthorized
Response::json_unauthorized(&serde_json::json!({"error": "Not authenticated"}))

// 403 Forbidden
Response::json_forbidden(&serde_json::json!({"error": "Access denied"}))

// 404 Not Found
Response::json_not_found(&serde_json::json!({"error": "Resource not found"}))

// 409 Conflict
Response::json_conflict(&serde_json::json!({"error": "Resource exists"}))

// 422 Unprocessable Entity
Response::json_unprocessable(&serde_json::json!({"error": "Validation failed"}))

// 429 Too Many Requests
Response::json_too_many_requests(&serde_json::json!({"error": "Rate limit exceeded"}))

// 500 Internal Server Error
Response::json_internal_error(&serde_json::json!({"error": "Server error"}))

// 501 Not Implemented
Response::json_not_implemented(&serde_json::json!({"error": "Not implemented"}))

// 503 Service Unavailable
Response::json_service_unavailable(&serde_json::json!({"error": "Service unavailable"}))
```

### Quick JSON Error Methods

```rust
// Quick error responses with simple messages
Response::json_error("Invalid input")
Response::json_not_found_msg("User not found")
Response::json_unauthorized_msg("Please log in")
Response::json_forbidden_msg("Access denied")
Response::json_internal_error_msg("Something went wrong")

// Validation errors with field-specific errors
let mut errors = HashMap::new();
errors.insert("email".to_string(), vec!["Invalid format".to_string()]);
errors.insert("password".to_string(), vec!["Too short".to_string()]);
Response::json_validation_error(errors)
```

## Available HTTP Status Codes

### 2xx Success
- `200 OK` - Successful request
- `201 Created` - Resource created successfully
- `202 Accepted` - Request accepted for processing
- `204 No Content` - Successful request with no body

### 3xx Redirection
- `301 Moved Permanently` - Permanent redirect
- `302 Found` - Temporary redirect
- `303 See Other` - Redirect after POST
- `304 Not Modified` - Conditional GET

### 4xx Client Errors
- `400 Bad Request` - Invalid request
- `401 Unauthorized` - Authentication required
- `403 Forbidden` - Insufficient permissions
- `404 Not Found` - Resource not found
- `409 Conflict` - Resource conflict
- `422 Unprocessable Entity` - Validation error
- `429 Too Many Requests` - Rate limiting

### 5xx Server Errors
- `500 Internal Server Error` - Server error
- `501 Not Implemented` - Feature not implemented
- `502 Bad Gateway` - Invalid gateway response
- `503 Service Unavailable` - Service temporarily unavailable
