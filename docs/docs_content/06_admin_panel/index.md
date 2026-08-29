# 🛡️ GritAdmin Panel - Developer Guide

GritAdmin provides an auto-generated administrative interface for your database tables. Simply annotate your repository with `#[derive(GritAdmin)]` and you get a complete CRUD admin panel with advanced filtering, inline editing, and a powerful query explorer.

---

## Getting Started

### 1. Configure Admin Credentials

Add the following to your `.env` file or change with your credentials:

```env
GRITSHIELD_ADMIN_USER=admin
GRITSHIELD_ADMIN_PASSWORD=gritshield2026
```

These credentials will be used to log into the admin panel at `/admin/login`.

---

### 2. Define Your Model

First, define your database entity using SeaORM:

```rust
// src/models/user.rs
use chrono::NaiveDateTime;
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
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

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
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
    grid_columns = ["id", "email", "username", "created_at", "status"], // Define table order
    read_only = ["id", "created_at"],         // Non-editable columns, by default `id` is not editable
)]
pub struct UserRepository {
    pub db: sea_orm::DatabaseConnection,
}
```
> [!IMPORTANT]
> GriAdmin expects your model to be at root `src/models/user.rs`, if not, define the model path in repository macro

```rust
#[repository(
    entity = "crate::module::model"
)]
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
![grid_columns](/docs/images/grid_columns.png)

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

![inline_edit](/docs/images/inline_edit.png)

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

![infinite_scroll](/docs/images/infinite_scroll.png)

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

![jql_explorer](/docs/images/query_explorer.png)

---

## Running the Admin Panel

2. **Start your server:**
    
    ```bash   
    cargo run OR cargo watch -w src -x run //Reloads on code changes
    ```
3. **Navigate to:** `http://localhost:8080/admin/login`
    
4. **Login** with the credentials you set in `.env`
    
5. **Explore** /admin/dashboard
    