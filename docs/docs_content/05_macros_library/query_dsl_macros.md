
# Query DSL Macros

JPA-like query dsl, at compile time, no extra overhead.

## `GritModel`

Defines your database entity with automatic repository generation and query DSL.

### Usage
```rust

use gritshield::{GritModel, GritRelation};
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize, GritModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub email: String,
    pub username: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}
```


### What It Generates

- **`find_by_{field}`** - Query by any field
    
- **`find_by_{field}_and_{field}`** - Multi-field queries (AND/OR)
    
- **`find_by_{field}_between`** - Range queries for numeric/date fields
    
- **`find_by_{field}_like`** - Pattern matching for string fields
    
- **`find_by_{field}_contains`** - Substring search
    
- **`find_by_{field}_gt`** / **`_lt`** - Greater/less than comparisons
    
- **`find_by_{field}_true`** / **`_false`** - Boolean filters
    
- **`exists_by_{field}`** - Existence check
    
- **`count_by_{field}`** - Count records
    
- **`delete_by_{field}`** - Delete by field
    

### Example: E-commerce User Management

```rust

#[derive(GritModel)]
#[sea_orm(table_name = "users")]
pub struct User {
    pub id: i64,
    pub email: String,
    pub status: String, // active, suspended, deleted
    pub tier: String,   // free, premium, enterprise
    pub created_at: NaiveDateTime,
}
// Auto-generated methods in repository
let user_repo = UserRepository { db };
// Find active premium users
let active_premium = user_repo
    .find_by_status_and_tier("active", "premium")
    .await?;
// Find users created in the last 30 days
let recent_users = user_repo
    .find_by_created_at_gt(Utc::now().naive_utc() - Duration::days(30))
    .await?;
// Find users with email containing domain
let gmail_users = user_repo
    .find_by_email_contains("@gmail.com")
    .await?;
// Count enterprises users
let enterprise_count = user_repo
    .count_by_tier("enterprise")
    .await?;
```

---

## `GritRelation`

Defines database relationships (HasMany, HasOne, BelongsTo) for automatic relation loading.

### Usage
```rust

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation, GritRelation)]
#[grit(table = "users")]
pub enum Relation {
    #[sea_orm(has_many = "super::post::Entity")]
    Post,
    #[sea_orm(has_many = "super::comment::Entity")]
    Comment,
}

// For belongs_to with custom foreign key
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation, GritRelation)]
#[grit(table = "posts")]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UserId",
        to = "super::user::Column::Id"
    )]
    User,
    #[sea_orm(has_many = "super::comment::Entity")]
    Comment,
}
```

### What It Generates

- **`with_{relation}`** - Load related data eagerly
    
- **`with_{relation}_nested`** - Deep nested loading with custom query builder
    

### Business Example: Social Media Platform

```rust
#[derive(GritRelation)]
#[grit(table = "users")]
pub enum UserRelation {
    #[sea_orm(has_many = "super::post::Entity")]
    Posts,
    #[sea_orm(has_many = "super::comment::Entity")]
    Comments,
    #[sea_orm(has_many = "super::follower::Entity")]
    Followers,
    #[sea_orm(has_many = "super::follower::Entity")]
    Following,
}

// Auto-generated relation loading
let user_with_data = user_repo
    .find_by_id(1)
    .with_posts()        // Load all posts
    .with_comments()     // Load all comments
    .with_followers()    // Load followers
    .with_following()    // Load who the user follows
    .await?;
// Deep nested relations (4 levels deep!)
let user_deep = user_repo
    .find_by_id(1)
    .with_posts_nested(|query| {
        query
            .with_comments_nested(|q| {
                q.with_user()  // Load comment author
            })
    })
    .with_followers_nested(|query| {
        query
            .with_posts()      // Followers' posts
            .with_comments()   // Followers' comments
    })
    .await?;
// Access nested data
for post in user_deep.posts.unwrap() {
    println!("Post: {}", post.content);
    for comment in post.comments.unwrap() {
        println!("  Comment by: {}", comment.user.unwrap().username);
    }
}
```

---

## `GritAdmin`

Defines how your data appears in the GritAdmin panel with zero frontend code.

### Usage

```rust

#[derive(GritAdmin)]
#[repository(
    searchable = ["email", "username"],       // Columns searchable via admin search
    grid_columns = ["id", "email", "username", "created_at", "status"],
    read_only = ["id", "created_at"],         // Non-editable columns
)]
pub struct UserRepository {
    pub db: sea_orm::DatabaseConnection,
}

// Make ALL columns read-only
#[repository(
    read_only = ["all"],
    // ...
)]
```

### What It Generates

Along side the admin panel features you get same methods used to build the admin panel

```rust
    // Clean syntax starts with query(), builds Select internally, 
    // giving you a quick column filters, sorting, offset, limits, ...
    let repo_user_query = user_repo
        .query()
        .where_gt(user::Column::Id, 3)
        .fetch()
        .await
        .unwrap();
```
    
---

## `GritSchema`

Defines request/response schemas for automatic OpenAPI/Swagger documentation.
- Once a struct annotated with GritSchema it's dynamically added to Swagger api docs, with it's data type.

```rust

#[derive(GritSchema, Deserialize)]
pub struct PaymentRequest {
    pub amount: f64,
    pub currency: String,
    pub source: String,           // card_id, bank_account_id
    pub description: Option<String>,
}

#[post("/api/payments", body = PaymentRequest)] // body required for built in Swagger api
pub async fn create_payment(
    ctx: RequestContext,
    payment_service: Arc<PaymentService>,
) -> Response {
    // Auto-deserialize from JSON
    let payment: PaymentRequest = ctx.json().await?;
    
    // Type-safe business logic
    let result = payment_service.charge(
        payment.amount,
        payment.currency,
        &payment.source,
        payment.description,
    ).await?;
    
    Response::json(200, &result)
}
```

---
