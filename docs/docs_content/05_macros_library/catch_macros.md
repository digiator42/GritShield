# Catch Macros

The `#[catch]` macro allows you to define custom error handlers for specific HTTP status codes, similar to error catching in other frameworks.

## Basic Usage

```rust
use gritshield::prelude::*;

#[catch(status = 404)]
async fn not_found_handler(ctx: RequestContext) -> Response {
    Response::not_found("Page not found")
}

#[catch(status = 500)]
async fn server_error_handler(ctx: RequestContext) -> Response {
    Response::internal_error("Something went wrong")
}
```

## Requirements

Catch handlers must:
- Be async functions
- Take `ctx: RequestContext` as a parameter
- Return `Response`

## Custom Error Pages

```rust
#[catch(status = 404)]
async fn custom_404(ctx: RequestContext) -> Response {
    Response::not_found(html! {
        div class="error-page" {
            h1 { "404 - Page Not Found" }
            p { "The page you're looking for doesn't exist." }
            a href="/" { "Go Home" }
        }
    })
}
```

## JSON Error Responses

```rust
#[catch(status = 401)]
async fn unauthorized_handler(ctx: RequestContext) -> Response {
    Response::json_unauthorized(&serde_json::json!({
        "error": "Authentication required",
        "code": "AUTH_REQUIRED"
    }))
}
```

## Multiple Status Codes

You can define catch handlers for different status codes:

```rust
#[catch(status = 400)]
async fn bad_request_handler(ctx: RequestContext) -> Response {
    Response::bad_request("Invalid request")
}

#[catch(status = 403)]
async fn forbidden_handler(ctx: RequestContext) -> Response {
    Response::forbidden("Access denied")
}

#[catch(status = 429)]
async fn rate_limit_handler(ctx: RequestContext) -> Response {
    Response::too_many_requests("Too many requests")
}
```

The catch handlers defined with `#[catch]` will be available globally in your application.