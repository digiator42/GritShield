
GritShield's router is built on a trie (prefix tree) for efficient path matching with O(n) complexity where n is the number of path segments.

## Basic Examples

```rust
#[get("/")]
async fn home(ctx: RequestContext) -> &'static str {
    "Home page"
}

// Static routes: `/users`, `/about`
#[post("/users")]
async fn create_user(ctx: RequestContext) -> ShieldResult<Response> {
    // Create user logic
    Ok(Response::redirect(303, "/users"))
}

// Dynamic parameters: `/users/:id`
#[get("/users/:id")]
async fn user(ctx: RequestContext) -> String {
    format!("User: {}", ctx.params.get("id").unwrap().as_str())
}

// Wildcard routes: /static/*path
#[get("/static/*path")]
async fn static_files(ctx: RequestContext) -> Response {
    let path = ctx.params.get("*path").unwrap().as_str();
    Response::static_file(&format!("static/{}", path))
}
```

## Route Priority

Routes are matched in the order they're added. Wildcards have lowest priority:

1. Exact matches (`/users/profile`)
2. Parameter matches (`/users/:id`)
3. Wildcard matches (`/docs/*path`)

## Manual Registration

```rust
router.add_route(HttpMethod::GET, "/health", |_| async { "OK" });
router.add_route(HttpMethod::POST, "/api/data", handle_api);
```