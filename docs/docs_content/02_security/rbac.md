GritShield provides RBAC through session user IDs and middleware.

## Session-Based RBAC

GritShield provides convenient built-in role checking methods directly on `RequestContext` for clean and secure authorization.

> ### First you need to store user roles in the session after login:
>
> 💡 ShieldResult<T> is `Result<T, ShieldError>`

```rust
use gritshield::routing::engine::ShieldResult;

#[post("/login")]
async fn login(ctx: RequestContext) -> ShieldResult<Response> {
    let creds: LoginDto = ctx.json()?;

    if authenticate(&creds) {
        ctx.login_user_id(&creds.user_id);
        ctx.set_session_data("role", "admin");
        Ok(Response::redirect(303, "/dashboard"))
    } else {
        Err(ShieldError::UnauthorizedAccess)
    }
}
```

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
pub async fn admin_panel(ctx: RequestContext) -> ShieldResult<Response> {
    // Or using strict guard with ?, Admin passes, Auditor will not!
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

- **But** `Operator` cann't access `Manager` or it's `childs` content, because they are on different branches!.

### Now use built-in functions inside your handlers

```rust
pub async fn manage_users(ctx: RequestContext) -> Response {
    // Check with inheritance tree
    if !ctx.has_role("Manager") {
        return Response::forbidden("Manager role or higher required");
    }

    // Strict version with ? (Recommended)
    ctx.require_role("Manager")?; // needs ShieldResult, or use unwrap

    // Handler logic...
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

impl Middleware for RoleMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {

        if let Some(role) = ctx.get_session_data("role") {
            if role == &self.required_role {
                return MiddlewareResult::Next(None);
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

Gritshield checks dyncamically for inheritance roles first if defined, falling back to fixed role checks, giving you zero boilerplate rbac helper.
