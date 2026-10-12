# Response

Every handler returns a [`Response`]. It is a plain struct holding the status
line, headers, cookies and a body:

```rust
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub cookies: Vec<Cookie>,
    pub body: ResponseBody,
}
```

The public API lives in `gritshield::http::response`; `Response` and the string
helpers are re-exported through the prelude:

```rust
use gritshield::prelude::*;
```

Everything else (`HttpStatus`, `JsonPayload`, ...) is imported from its module.

## One API, three payload families

`Response::build(status, payload)` is the single polymorphic constructor. What
the *payload* can be is decided by the [`IntoResponseBody`] trait, which picks
both the body variant and its implied `Content-Type`. The status helpers below
(`ok`, `bad_request`, `json_ok`, ...) are thin sugar over it.

The three payload families:

| Family | Payload type | `Content-Type` |
|--------|--------------|----------------|
| HTML   | `SafeHtml`, `String`, `&str`, `maud::PreEscaped<String>` | `text/html` |
| JSON   | `JsonPayload<T>`, `HashMap<K, V>` (owned or borrowed) | `application/json` |
| Binary | `static_file(path)` | from file extension |

### Text and HTML

Plain `String` / `&str` payloads are trusted and sent as `text/html`:

```rust
Response::ok("Operation completed successfully.")
```

To be explicit about a value being safe to render, wrap it with the sanitizer:

```rust
let markup = Sanitizer::trust("<h1>Validation Error</h1><p>Invalid credentials</p>");
Response::bad_request(markup)
```

Maud templates are the idiomatic way to build HTML. `html!` produces a `Markup`
you render through `.into_string()`, or you can hand the response a
`maud::PreEscaped<String>` when the content is already compiled:

```rust
Response::forbidden(html! { p { "You do not have permission." } }.into_string())
```

When the HTML is already compiled, `maud::PreEscaped<String>` is accepted
directly (requires `maud` in your `Cargo.toml`):

```rust
Response::ok(maud::PreEscaped("<h1>Bold</h1>".to_string()))
```

> Only `PreEscaped<String>` and `SafeHtml` are `IntoResponseBody`. A raw
> `Markup` is **not** — call `.into_string()` first.

### JSON

`JsonPayload<T>` is a thin wrapper that routes any `Serialize` value to the JSON
body variant:

```rust
use gritshield::http::response::JsonPayload;

#[derive(serde::Serialize)]
struct ErrorContainer { error: String, missing_fields: Vec<String> }

let details = ErrorContainer { error: "Missing form values".into(), missing_fields: vec!["username".into()] };
Response::bad_request(JsonPayload(details))
```

`HashMap`s (owned or borrowed) serialise to JSON directly without a wrapper:

```rust
use std::collections::HashMap;

Response::bad_request(&HashMap::from([("status", "failed")]))
```

Prefer the dedicated JSON constructor when you also control the status
(see below) — it needs no wrapper and keeps the `Content-Type` in one place:

```rust
Response::json(HttpStatus::BadRequest, &ErrorContainer { error: "bad".into(), missing_fields: vec![] })
```

### Static files

```rust
Response::static_file("public/index.html")
```

## Status helpers (HTML / general)

| Method | HTTP status | Use case |
|--------|-------------|----------|
| `Response::ok(payload)`        | 200 | Success |
| `Response::created(payload)`   | 201 | Resource created |
| `Response::bad_request(payload)`   | 400 | Validation / client error |
| `Response::unauthorized(payload)`  | 401 | Authentication required |
| `Response::forbidden(payload)`     | 403 | Insufficient permissions |
| `Response::not_found(payload)`     | 404 | Missing resource |
| `Response::conflict(payload)`      | 409 | Resource conflict |
| `Response::too_many_requests(payload)` | 429 | Rate limited |
| `Response::internal_error(payload)` | 500 | Server fault |

## JSON responses

### Typed status constructors

```rust
Response::json_ok(&serde_json::json!({"status": "success"}))          // 200
Response::json_created(&serde_json::json!({"id": 123}))               // 201
Response::json_accepted(&serde_json::json!({"message": "processing"}))// 202
Response::json_no_content()                                           // 204

Response::json_bad_request(&serde_json::json!({"error": "invalid input"}))    // 400
Response::json_unauthorized(&serde_json::json!({"error": "sign in"}))         // 401
Response::json_forbidden(&serde_json::json!({"error": "denied"}))             // 403
Response::json_not_found(&serde_json::json!({"error": "gone"}))               // 404
Response::json_conflict(&serde_json::json!({"error": "exists"}))              // 409
Response::json_unprocessable(&serde_json::json!({"error": "bad body"}))       // 422
Response::json_too_many_requests(&serde_json::json!({"error": "slow down"}))  // 429
Response::json_internal_error(&serde_json::json!({"error": "boom"}))          // 500
Response::json_not_implemented(&serde_json::json!({"error": "todo"}))         // 501
Response::json_service_unavailable(&serde_json::json!({"error": "busy"}))     // 503
```

Every `*_msg` variant is a shorthand that wraps a message in
`{"error": "message"}`:

```rust
Response::json_error("Invalid input")          // 400
Response::json_not_found_msg("No such user")   // 404
Response::json_unauthorized_msg("Please log in")  // 401
Response::json_forbidden_msg("Access denied")     // 403
Response::json_internal_error_msg("Something went wrong") // 500
```

Field-level validation errors are wrapped in `{"errors": {field: [msg]}}`
at `422`:

```rust
Response::json_validation_error(HashMap::from([
    ("email".to_string(), vec!["invalid format".to_string()]),
]))
```

### `HttpStatus` enum

Use `Response::json` (or `json_with`) with the typed status enum instead of a
raw code:

```rust
use gritshield::http::response::HttpStatus;

Response::json(HttpStatus::Accepted, &payload)
```

`HttpStatus` mirrors the status helpers: `Ok`, `Created`, `Accepted`,
`NoContent`, `MovedPermanently`, `Found`, `SeeOther`, `NotModified`,
`BadRequest`, `Unauthorized`, `Forbidden`, `NotFound`, `MethodNotAllowed`,
`Conflict`, `UnprocessableEntity`, `TooManyRequests`, `InternalServerError`,
`NotImplemented`, `BadGateway`, `ServiceUnavailable`.

## Headers

Add single or multiple headers on any response:

```rust
Response::ok("Success")
    .with_header("Cache-Control", "no-cache")
    .with_header("X-Request-Id", request_id);

Response::ok("Success")
    .with_headers(vec![
        ("X-A".to_string(), "1".to_string()),
        ("X-B".to_string(), "2".to_string()),
    ]);
```

Every response also ships security defaults controlled by the framework:
`X-Content-Type-Options: nosniff` and `X-Frame-Options: DENY`.

## Redirects

```rust
Response::redirect(303, "/dashboard")   // status + Location + body
Response::navigate_to("/login")         // 302 Found
```

## Cookies

`Cookie` defaults are locked down: 1-hour `Max-Age`, `HttpOnly`, `Secure`,
`SameSite=Strict`. Relax them only for development:

```rust
let session = Cookie::new("session_id", "abc123");
let local = Cookie::new("session_id", "abc123")
    .set_secure(false)
    .set_same_site(SameSite::Lax);

Response::ok("Login successful").with_cookie(local);
```

For signed and tamper-resistant cookies use `CookieJar`
(`gritshield::security::cookies`) — see [Security](../03_security/index.md).

## Server-Sent Events

`Response::sse` streams a live [`SseStream`] polled from a broadcast channel.
The handler returns immediately; frames pushed afterwards reach every attached
client. The response carries the mandated event-stream headers (no caching, no
proxy buffering, keep-alive).

```rust
use gritshield::http::SseStream;

#[get("/events")]
async fn events(_ctx: RequestContext) -> Response {
    let stream = SseStream::new();          // 15s keep-alive pings
    if let Ok(n) = stream.send_json("tick", &serde_json::json!({"seq": 1})) {
        debug!("{n} listeners");
    }
    Response::sse(stream)
}
```

`SseStream` is `Clone`, so unrelated tasks can push frames without owning the
response. Use `queue_initial(event, data)` to guarantee a frame reaches the
wire before any live frame (needed for handshakes). Slow consumers are lagged
(dropped frames), never allowed to pin memory.

## Server-side notes

- Status lines are serialised with the correct reason phrase (`404 Not Found`,
  not a blanket `OK`).
- An explicit `Content-Type` header set on the `Response` overrides the one
  implied by the body variant.
- Streaming bodies omit `Content-Length` and are pumped onto the socket; the
  `Response` writer handles this automatically.

[`IntoResponseBody`]: https://docs.rs/gritshield
[`SseStream`]: https://docs.rs/gritshield