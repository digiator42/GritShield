# 🛡️ GritAdmin Panel - Developer Guide

GritAdmin provides an auto-generated administrative interface for your database tables. Simply annotate your repository with `#[derive(GritAdmin)]` and you get a complete CRUD admin panel with advanced filtering, inline editing, and a powerful query explorer.

---

## Getting Started

### 1. Configure Admin Credentials

Add the following to your `.env` file:

```env
GRITSHIELD_ADMIN_USER=admin
GRITSHIELD_ADMIN_PASSWORD=gritshield2026
```

These credentials will be used to log into the admin panel at `/admin/login`.

---

### 2. Define Your Model

First, define your database entity using SeaORM with the `GritModel` and `GritRelation` macros:

```rust
// src/models/user.rs
use chrono::NaiveDateTime;
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
    pub status: String,  // active, suspended, deleted
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation, GritRelation)]
#[grit(table = "users")]
pub enum Relation {
    #[sea_orm(has_many = "super::post::Entity")]
    Posts,
}
```

---
### 3. Create the Repository

Now create a repository struct and annotate it with `#[derive(GritAdmin)]` and the `#[repository]` attribute:

```rust
// src/repositories/user_repository.rs
use gritshield::GritAdmin;
#[derive(GritAdmin)]
#[repository(
    searchable = ["email", "username"],       // Columns searchable via admin search
    grid_columns = ["id", "email", "username", "created_at", "status"],
    read_only = ["id", "created_at"],         // Non-editable columns
)]
pub struct UserRepository {
    pub db: sea_orm::DatabaseConnection,
}
```
---

## Repository Attributes Explained

### `searchable`

Columns that will be searchable via the global search bar.

```rust
searchable = ["email", "username", "phone"]
```
### `grid_columns`

Controls which columns appear in the table and their display order. Columns are rendered in the exact sequence defined.

```rust
grid_columns = ["id", "email", "username", "created_at", "status"]
```
![grid_columns](/images/grid_columns.png)

### `read_only`

Prevents certain columns from being edited inline.

```rust
read_only = ["id", "created_at", "updated_at"]
```

**Make ALL columns read-only:**

```rust
read_only = ["all"]
```

---
## Inline Editing

Any editable field (not in `read_only`) can be modified directly in the grid:

1. **Click** on any editable cell
    
2. **Type** the new value
    
3. **Press Enter** to save
    

Changes are automatically persisted.

![inline_edit](/images/inline_edit.png)

---

## Advanced Filters

Each grid column can be filtered using advanced operators:

|Operator|Description|
|---|---|
|`contains`|Text contains value|
|`eq`|Equal to|
|`ne`|Not equal to|
|`gt`|Greater than|
|`gte`|Greater than or equal|
|`lt`|Less than|
|`lte`|Less than or equal|
|`startswith`|Text starts with|
|`endswith`|Text ends with|
|`is_null`|Field is NULL|
|`is_not_null`|Field is NOT NULL|

---

## Pagination Options

GritAdmin supports two pagination modes:

### 1. Infinite Scroll

Default, Rows load automatically as you scroll down. Perfect for browsing large datasets.

### 2. Standard Pagination

Traditional page-by-page navigation with page numbers.

**Toggle between modes** using the "Navigation" dropdown in the filter bar.

![infinite_scroll](/images/infinite_scroll.png)

---

## Query Explorer

GritAdmin includes query explorer. This allows you to run complex SQL-like queries directly from the admin panel.

### Basic Syntax

```sql
SELECT column1, column2 FROM table_name WHERE condition
```
### Examples

**Simple SELECT:**

```sql
SELECT id, email, username FROM users WHERE status = 'active'
```

**JOIN Query:**

```sql

SELECT users.username, posts.title FROM users JOIN posts ON users.id = posts.user_id WHERE posts.status = 'published'
```

**Filtering:**

```sql
SELECT id, email, created_at FROM users WHERE created_at > '2024-01-01'
```

![jql_explorer](/images/query_explorer.png)

---

## Complete Example

Here's a complete example with a User and Post relationship:

### Models

```rust
// src/models/user.rs
#[sea_orm(table_name = "users")]
pub struct User {
    pub id: i64,
    pub email: String,
    pub username: String,
    pub status: String,
    pub created_at: NaiveDateTime,
}

pub enum UserRelation {
    #[sea_orm(has_many = "super::post::Entity")]
    Posts,
}
// src/models/post.rs
#[sea_orm(table_name = "posts")]
pub struct Post {
    pub id: i64,
    pub user_id: i64,
    pub title: String,
    pub content: String,
    pub status: String,
    pub created_at: NaiveDateTime,
}

pub enum PostRelation {
    #[sea_orm(belongs_to = "super::user::Entity")]
    User,
}
```

### Repositories

```rust
// src/repositories/user_repository.rs
#[derive(GritAdmin)]
#[repository(
    searchable = ["email", "username", "status"],
    grid_columns = ["id", "username", "email", "status", "created_at"],
    read_only = ["id", "created_at"], // By default id is not editable
)]
pub struct UserRepository {
    pub db: sea_orm::DatabaseConnection,
}
// src/repositories/post_repository.rs
#[derive(GritAdmin)]
#[repository(
    searchable = ["title", "content", "status"],
    grid_columns = ["id", "user_id", "content", "created_at"],
    read_only = ["created_at"],
)]
pub struct PostRepository {
    pub db: sea_orm::DatabaseConnection,
}
```
---

## Running the Admin Panel

2. **Start your server:**
    
    ```bash   
    cargo run OR cargo watch -w src -x run //Reloads on code changes
    ```
3. **Navigate to:** `http://localhost:8080/admin/login`
    
4. **Login** with the credentials you set in `.env`
    
5. **Explore** /admin/dashboard
    