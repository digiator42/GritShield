# 🌐 HTTP Macros

This is a quick reference of GritShield HTTP macros that combine **routing** with **compile-time dependency injection** and **OpenAPI documentation**.

you need to check:

- **Dependency Injection**
- **OpenAPI / Swagger**

## `#[get]`

### Basic Usage

```rust

#[get("/api/users")]
pub async fn list_users(ctx: RequestContext) -> Response {
    // Simple route
    Response::ok("List of users")
}
```

### With Path Parameters

```rust

#[get("/api/users/:id")]
pub async fn get_user(ctx: RequestContext) -> Response {
    // You must add * to get the dynamic param id
    let id = ctx.params.get("*id").unwrap().as_str();
    // Fetch user by id
    Response::ok(format!("User ID: {}", id))
}
```

### With Query Parameters

```rust
#[get("/api/users")]
pub async fn list_users(ctx: RequestContext) -> Response {
    let page = ctx.query.get("page").unwrap_or("1");
    let limit = ctx.query.get("limit").unwrap_or("10");

    Response::ok(format!("Page: {}, Limit: {}", page, limit))
}
```

### With Dependency Injection

```rust

#[get("/api/users")]
pub async fn list_users(
    ctx: RequestContext,
    user_service: Arc<UserService>,  // Auto-injected from DI container
    db: Arc<DatabasePool>,            // Auto-injected
) -> Response {
    let users = user_service.list_all().await;
    Response::json(200, &users)
}
```

### With Role-Based Access Control

```rust

#[get("/api/admin/users", role = "Admin")]
pub async fn admin_list_users(ctx: RequestContext) -> Response {
    // Only accessible to users with "Admin" role
    Response::ok("Admin user list")
}
```
---

## `#[post]`

### Basic Usage

```rust

#[post("/api/users")]
pub async fn create_user(ctx: RequestContext) -> Response {
    // Manual JSON deserialization
    let data: serde_json::Value = ctx.json_body().await?;
    Response::json(201, &data)
}
```

### With Request Body Schema

```rust

use gritshield::GritSchema;
use serde::Deserialize;

#[derive(GritSchema, Deserialize)]
pub struct CreateUserRequest {
    pub email: String,
    pub username: String,
    pub password: String,
    pub tier: Option<String>,
}

#[post("/api/users", body = CreateUserRequest)]
pub async fn create_user(
    ctx: RequestContext,
    user_service: Arc<UserService>,
) -> Response {
    // Auto-deserialized from JSON
    let data: CreateUserRequest = ctx.json::<CreateUserRequest>().await?;

    let user = user_service.create_user(data).await.unwrap();
    Response::json(201, &user)
}
```

### With Dependency Injection

```rust

#[post("/api/orders", body = CreateOrderRequest)]
pub async fn create_order(
    ctx: RequestContext,
    order_service: Arc<OrderService>,
    payment_service: Arc<PaymentService>,
    notification_service: Arc<NotificationService>,
) -> Response {
    let request: CreateOrderRequest = ctx.json::<CreateUserRequest>().await?;

    // All services auto-injected!
    let order = order_service.create(request).await?;
    payment_service.process(order.total).await?;
    notification_service.send_confirmation(&order).await?;

    Response::json(201, &order)
}
```

### With Role Protection

```rust

#[post("/api/admin/users", role = "Admin", body = CreateUserRequest)]
pub async fn admin_create_user(
    ctx: RequestContext,
    admin_service: Arc<AdminService>,
) -> Response {
    // Only Admins can create users via this endpoint
    let data: CreateUserRequest = ctx.json::<CreateUserRequest>().await?;
    Response::json(201, &admin_service.create_user(data).await?)
}
```
---

## `#[put]`

### Basic Usage

```rust

#[put("/api/users/:id")]
pub async fn update_user(ctx: RequestContext) -> Response {
    let id = ctx.params.get("id").unwrap().as_str();
    let data: serde_json::Value = ctx.json().await?;

    Response::ok(format!("Updated user {}: {}", id, data))
}
```
---

## `#[patch]`

### Basic Usage

```rust

#[patch("/api/users/:id")]
pub async fn partial_update_user(ctx: RequestContext) -> Response {
    let id = ctx.params.get("id").unwrap().as_str();
    let data: serde_json::Value = ctx.json().await?;

    Response::ok(format!("Partially updated user {}: {}", id, data))
}
```
---

## `#[delete]`

### Basic Usage

```rust

#[delete("/api/users/:id")]
pub async fn delete_user(ctx: RequestContext) -> Response {
    let id = ctx.params.get("*id").unwrap().as_str();
    Response::ok(format!("Deleted user {}", id))
}
```
---

## `#[controller]`

Groups multiple routes under a common base path with automatic DI.

### Basic Usage

```rust

#[controller("/api/users")]
impl UserController {
    #[get("/")]
    pub async fn list(&self, ctx: RequestContext) -> Response {
        Response::ok("List all users")
    }

    #[get("/:id")]
    pub async fn get(&self, ctx: RequestContext) -> Response {
        let id = ctx.params.get("id").unwrap().as_str();
        Response::ok(format!("Get user {}", id))
    }

    #[post("/", body = CreateUserRequest)]
    pub async fn create(&self, ctx: RequestContext) -> Response {
        let data: CreateUserRequest = ctx.json().await?;
        Response::json(201, &data)
    }
}
```

### With Dependency Injection in Controller Fields

```rust

#[derive(GritComponent)]
pub struct UserController {
    pub user_service: Arc<UserService>,
    pub audit_service: Arc<AuditService>,
}
#[controller("/api/users")]
impl UserController {
    #[get("/")]
    pub async fn list(&self, ctx: RequestContext) -> Response {
        // user_service and audit_service are auto-injected
        let users = self.user_service.list_all().await?;
        self.audit_service.log_action("list_users").await?;
        Response::json(200, &users)
    }

    #[post("/", body = CreateUserRequest)]
    pub async fn create(
        &self,
        ctx: RequestContext,
        email_service: Arc<EmailService>, // Additional DI!
    ) -> Response {
        let data: CreateUserRequest = ctx.json().await?;

        let user = self.user_service.create_user(data).await?;
        email_service.send_welcome(&user.email).await?;
        self.audit_service.log_action("create_user").await?;

        Response::json(201, &user)
    }
}
```
---

## `#[action]`

Adds custom actions to the GritAdmin panel.

### Usage

```rust

#[action(
    table = "post",
    label = "Publish",
    icon = "📢",
    color = "text-emerald-400"
)]
async fn publish_posts(ctx: RequestContext) -> Response {
    let ids = ctx.form.fields.get("ids").unwrap().split(',');
    let db = ctx.db.clone().unwrap();

    for id in ids {
        let id = id.parse::<i64>()?;
        let sql = format!("UPDATE posts SET status = 'published' WHERE id = {}", id);
        let stmt = sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Sqlite,
            sql,
        );
        db.execute(stmt).await?;
    }

    Response::ok("Posts published successfully")
}
```
