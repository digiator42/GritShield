GritShield provides RBAC through session user IDs and middleware.

## Session-Based RBAC

GritShield provides convenient built-in role checking methods directly on `RequestContext` for clean and secure authorization.

> ### First you need to store user roles in the session after login:
>
> 💡 ShieldResult<T> is `Result<T, ShieldError>`

```rust
use gritshield::routing::engine::ShieldResult;
use gritshield::security::errors::ShieldError;

#[post("/login")]
async fn login(ctx: RequestContext) -> ShieldResult<Response> {
    let body = ctx
        .json_body()
        .await
        .ok_or_else(|| ShieldError::BadRequest("invalid JSON".into()))?;
    let user_id = body.get("user_id").and_then(|v| v.as_str()).unwrap_or("");

    ctx.login_user_id(user_id);
    ctx.set_session_data("role", "Admin");
    Ok(Response::redirect(303, "/dashboard"))
}
```

Role strings are **case-sensitive** — `"Admin"` and `"admin"` are different
roles. The login handler is where you choose the exact string every later check
must use. `ctx.json::<T>().await?` is also available, but `T` must derive
`GritSanitizable` (`#[derive(GritSanitizer)]`).

## Now you can use built-in ctx methods

```rust
// Check auth
pub async fn dashboard(ctx: RequestContext) -> Response {
    // Expects `user_id` in session or jwt claims
    if !ctx.is_user_authenticated() {
        return Response::unauthorized("Please log in to access this page.");
    }
    // ...
}
```

## Fixed Role Checking (with Hierarchy)

Rust

```rust
pub async fn admin_panel(ctx: RequestContext) -> Response {
    // Simple check
    if !ctx.has_role("Admin") {
        return Response::forbidden("Admin access required");
    }

    // Admin logic here...
    Response::ok("Welcome Admin")
}

// Simpler
pub async fn operator_panel(ctx: RequestContext) -> ShieldResult<Response> {
    // Strict guard: Admin passes through inheritance, Auditor does not.
    ctx.require_role("Operator")?;

    Ok(Response::ok("Welcome Operator"))
}
```

## Dynamic Role Inheritance

GritShield Allows you to define hierarchical relationships between roles (e.g., `Admin` inherits from `Manager`, which inherits from `Editor`).

This system enables flexible and maintainable permission structures without hardcoding every role combination.

## Key Benefits

- Roles can inherit permissions from other roles
- Recursive checking (multi-level inheritance)
- Clean separation between fixed roles and dynamic hierarchies
- Easy to extend or modify role relationships at runtime

## Example Usage

You can configure role inheritance when building your router:

```rust
let router = Router::new()
    .add_role_inheritance("Admin", vec!["Manager", "Operator", "Auditor"])
    .add_role_inheritance("Manager", vec!["Editor", "Viewer"])
    .add_role_inheritance("Editor", vec!["Contributor"]);
```

How It Works

- `Admin` inherits everything (`Manager`, `Operator`, `Auditor`)
- `Manager` inherits `Editor` and `Viewer`
- Inheritance is recursive — so `Admin` automatically has access to `Editor` and `Contributor` as well.

```
                               [Admin]
                             /    |    \
                            /     |     \
                           /      |      \
                    [Manager] [Operator] [Auditor]
                       /   \
                      /     \
                     [Editor] [Viewer]
                        |
                     [Contributor]
```

- **But** `Operator` cannot access `Manager` or its children's content, because they are on different branches.

### Now use built-in functions inside your handlers

```rust
pub async fn manage_users(ctx: RequestContext) -> ShieldResult<Response> {
    // Check with inheritance tree
    if !ctx.has_role("Manager") {
        return Err(ShieldError::Forbidden);
    }

    // Strict version with ? (Recommended)
    ctx.require_role("Manager")?;

    // Handler logic...
    Ok(Response::ok("managed"))
}

pub async fn edit_post(ctx: RequestContext) -> ShieldResult<Response> {
    // This will return true for: Editor, Manager, Admin
    ctx.require_role("Editor")?;

    // Only users with Editor role or higher can proceed
    Ok(Response::ok("Post edited successfully"))
}
```

## Create Your Own Role Check Middleware

Create custom middleware for role validation:

```rust
struct RoleMiddleware {
    required_role: String,
}

#[async_trait]
impl Middleware for RoleMiddleware {
    async fn on_request(&self, ctx: &mut RequestContext) -> MiddlewareResult {

        if let Some(role) = ctx.get_session_data("role") {
            if role == &self.required_role {
                return MiddlewareResult::Next(None);
            }
        }

        MiddlewareResult::Error(Response::forbidden(
            Sanitizer::trust("<h1>403 Forbidden - Insufficient Role</h1>")
        ))
    }
}

// Usage
router = router
    .add_middleware(AuthMiddleware::new_session(vec![], Some("/login")))
    .add_middleware(RoleMiddleware { required_role: "admin".to_string() });
```

## JWT-Based RBAC

Embed roles in JWT claims:

```rust
#[derive(Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,
    pub role: String,
    pub exp: usize,
}

// Create token with role
let claims = Claims {
    sub: user_id,
    role: "admin".to_string(),
    exp: expiration,
};
let token = jwt_handler.sign(&claims)?;

// Access role in handler
#[get("/admin")]
async fn admin_panel(ctx: RequestContext) -> Response {
    let role = ctx.claims.as_ref().map(|c| c.role.as_str()).unwrap_or("");
    if role != "admin" {
        return Response::new(403, Sanitizer::trust("Forbidden"));
    }
    // Admin logic...
}
```

## Now let's save you from writing RBAC functions each time

You can protect routes directly at the handler definition using the `role` parameter:

```rust
#[post("/dashboard", role = "Admin")]
pub async fn admin_panel(ctx: RequestContext) -> Response {
    // handler
    Response::ok("Hello Admin")
}
```

GritShield checks the dynamic inheritance tree first if one is defined, then
falls back to the fixed role checks, giving you zero-boilerplate RBAC.

The `role` attribute is enforced in the connection loop *before* your handler is
called (`src/http/connection.rs`), so a handler cannot forget it. A failure is
answered with:

```json
{ "error": "Access Denied: Missing required operational role clearance 'Admin'." }
```

## Runnable example

[`examples/rbac_caps`](https://github.com/digiator42/GritShield/tree/main/examples/rbac_caps)
logs in as five roles and measures the whole matrix - role attribute, inline
`require_role` guard and capability tokens - with no database:

```bash
cargo run --manifest-path examples/rbac_caps/Cargo.toml
```

`RequestContext` also exposes `has_fixed_role`, which is *not* the tree: it
applies a hardcoded `Admin > Operator > Auditor` ladder and is not used by the
route guards. Prefer `has_role` with an explicit `add_role_inheritance` tree, as
the example's `main.rs` does. Literal role `"SuperAdmin"` passes every `has_role`
check.
