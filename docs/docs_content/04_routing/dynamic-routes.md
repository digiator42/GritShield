
## Single Parameter `:param`

Define routes with single segment parameters:

```rust
#[get("/users/:id")]
async fn get_user(ctx: RequestContext) -> String {
    let user_id = ctx.params.get("id").unwrap().as_str();
    format!("User ID: {}", user_id)
}

#[get("/posts/:slug")]
async fn get_post(ctx: RequestContext) -> String {
    let slug = ctx.params.get("slug").unwrap().as_str();
    format!("Post: {}", slug)
}
```

Access parameters via `ctx.params` HashMap.

## Multiple Parameters

```rust
#[get("/users/:user_id/posts/:post_id")]
async fn get_user_post(ctx: RequestContext) -> String {
    let user_id = ctx.params.get("user_id").unwrap().as_str();
    let post_id = ctx.params.get("post_id").unwrap().as_str();
    format!("User {} Post {}", user_id, post_id)
}
```

## Wildcard/Catch-All `*param`

Capture multiple path segments:

```rust
#[get("/docs/:*path")]
async fn documentation(ctx: RequestContext) -> String {
    let path = ctx.params.get("*path").unwrap().as_str();
    format!("Documentation path: {}", path)
}
```

Examples:

- `/docs` → path = "" (empty string)
- `/docs/guide` → path = "guide"
- `/docs/guide/getting-started` → path = "guide/getting-started"


## File‑Based Routing (Next.js style)

Place your handlers in `src/pages/`:

- `src/pages/index.rs` → route `/`
- `src/pages/users/[id].rs` → route `/users/:id`
- `src/pages/api/[..path].rs` → route `/api/**` (catch‑all)

Inside the file, use the `register_page!` macro:

```rust
use gritshield::prelude::*;

register_page!(HttpMethod::GET, |_| async { "Hello from file route" });
```

GritShield automatically discovers `.rs` files under `src/pages` and mounts them.

```rust
let router = Router::new()
    .mount_file_routes("src/pages")
```

## Parameter Validation

```rust
#[get("/users/:id")]
async fn get_user(ctx: RequestContext) -> Result<Response, ShieldError> {
    let id_str = ctx.params.get("id").unwrap().as_str();
    let user_id: i32 = id_str.parse()
        .map_err(|_| ShieldError::FormParsingError("Invalid user ID".into()))?;
    
    // Use user_id
    Ok(Response::json(200, &user))
}
```

## Optional/Query Parameters

For optional parameters, define separate routes or use wildcards:

```rust
#[get("/search")]
async fn search(ctx: RequestContext) -> String {
    let query = ctx.query.get("q").map(|v| v.as_str()).unwrap_or("");
    format!("Searching for: {}", query)
}
```