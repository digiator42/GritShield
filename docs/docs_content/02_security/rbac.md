# Role-Based Access Control (RBAC)

GritShield provides RBAC through session user IDs and middleware.

## Session-Based RBAC

Store user roles in the session after login:

```rust
#[post("/login")]
async fn login(ctx: RequestContext) -> Result<Response, FrameworkError> {
    let creds: LoginDto = ctx.json()?;

    if authenticate(&creds) {
        ctx.login_user_id(&creds.user_id);
        ctx.set_session_data("role", "admin");
        Ok(Response::redirect(303, "/dashboard"))
    } else {
        Err(FrameworkError::UnauthorizedAccess)
    }
}
```

## Role Check Middleware

Create custom middleware for role validation:

```rust
struct RoleMiddleware {
    required_role: String,
}

impl Middleware for RoleMiddleware {
    fn execute(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        if let Some(ref session) = ctx.session {
            let session = session.lock().unwrap();
            if let Some(role) = session.data.get("role") {
                if role == &self.required_role {
                    return MiddlewareResult::Next(None);
                }
            }
        }

        MiddlewareResult::Error(Response::new(403,
            Sanitizer::trust("<h1>403 Forbidden - Insufficient Role</h1>")))
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

## Route-Specific Roles

Use custom attributes or manual checks:

```rust
#[get("/api/admin/users")]
async fn admin_users(ctx: RequestContext) -> Response {
    if !has_role(&ctx, "admin") {
        return Response::new(403, Sanitizer::trust("Forbidden"));
    }
    // Handler logic
}

fn has_role(ctx: &RequestContext, required: &str) -> bool {
    ctx.get_session_data("role").map_or(false, |r| r == required)
}
```
