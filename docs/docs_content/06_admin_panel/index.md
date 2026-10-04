# 🛡️ GritAdmin Panel - Developer Guide

GritAdmin provides an auto-generated administrative interface for your database tables. Simply annotate your repository with `#[derive(GritAdmin)]` and you get a complete CRUD admin panel with advanced filtering, inline editing, and a powerful query explorer.

> A runnable version of this page lives in
> [`examples/admin_panel`](https://github.com/digiator42/GritShield/tree/main/examples/admin_panel) -
> four small files, starts with no configuration, and its README lists the
> failure modes that are easy to hit here (module paths, `mount_db`, read-only
> columns, the credentials fallback).

---

## Getting Started

### 0. Enable the Feature

The panel is behind the `admin` cargo feature, plus a driver for your database:

```toml
[dependencies]
gritshield = { version = "0.2", features = ["admin", "sqlite"] }
```

There is no `mount_admin()` to call. `Router::new()` mounts the panel routes when
the feature is on.

### 1. Configure Admin Credentials

Add the following to your `.env` file or change with your credentials:

```env
GRITSHIELD_ADMIN_USER=admin
GRITSHIELD_ADMIN_PASSWORD=gritshield2026
```

These credentials will be used to log into the admin panel at `/admin/login`.

> [!WARNING]
> If either variable is missing, a random 12-character password is generated
> instead. In development it is printed to stdout; when `APP_ENV=production` it
> is **not** printed, so the panel is unreachable until you set the variables and
> restart.

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

Adding `GritModel` and `GritRelation` to the entity registers the model's
columns with the schema registry and generates the repository query methods -
`find_by_email_like`, `find_by_created_at_gt`, and so on. The panel works
without them; what you lose is the schema-driven extras such as foreign-key
links between grids.

See `examples/admin_panel/src/models/product.rs` for the annotated version.

---
### 3. Create the Repository

Now create a repository struct and annotate it with `#[derive(GritAdmin)]` and the `#[repository]` attribute:

```rust
// src/repositories/user.rs
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
> Both derives *derive* the module paths they need, and the error you get when
> you guess wrong points at the derive rather than at the path:
>
> ```text
> GritModel  on models/user.rs        -> crate::repositories::user::UserRepository
> GritAdmin  on repositories/user.rs -> crate::models::user
> ```
>
> So the table `users` maps to module `user` on both sides, and the repository
> file has to be `src/repositories/user.rs` - not `user_repository.rs`, which
> compiles until you add a `GritModel` anywhere in the crate and then fails with
> `cannot find 'repositories' in the crate root`.
>
> Both accept an override, and the value must be a **string literal** - a bare
> path is rejected by the attribute parser:
>
> ```rust
> #[repository(
>     entity = "crate::models::user"
> )]
> ```
>
> ```rust
> #[grit(repository = "crate::repositories::user::UserRepository")]
> ```

### 4. Mount the Database

Two calls, both required:

```rust
let db = DbManager::connect(DbConfig::default()).await?;

// The table the panel writes every edit and delete into. Skip this and the grid
// looks correct right up until the first save, which then fails.
gritshield::database::db::ensure_internal_audit_log_table(&db).await?;

let router = Router::new()
    .mount_db(db);
```

`mount_db` is not optional. Without it `ctx.db` is `None` and every panel
handler panics on `.expect("DB connection missing")`.

### 5. Know What Gets Mounted

Per registered model:

|Method|Path|Does|
|---|---|---|
|`GET`|`/admin/{model}`|grid, filters, pager|
|`GET`|`/admin/{model}/search`|cross-model search palette|
|`GET`|`/admin/{model}/query-explorer`|ad-hoc filter builder|
|`GET`|`/admin/{model}/:id`|single record|
|`PATCH`|`/admin/{model}/update-cell`|inline cell edit|
|`DELETE`|`/admin/{model}/delete`|delete one|
|`POST`|`/admin/{model}/bulk-delete`|delete many|
|`POST`|`/admin/{model}/bulk-create`|insert many|
|`GET`|`/admin/{model}/export`|CSV of the current filter|

`{model}` is the repository name minus `Repository`, lowercased -
`UserRepository` gives `/admin/user`, even though the table is `users`. The same
slug is what the grid uses for every link it renders.

Plus the static surface: `/admin/dashboard`, `/admin/login`, `/admin/metrics`,
`/admin/auditlog`, `/admin/rbac-matrix`, `/admin/settings/security`,
`/admin/api/search-palette`, `/admin/api/create-table`,
`/admin/api/alter-table/:table_slug/add-column`.

These handlers go on the **global** middleware stack, so enabling `admin` adds a
middleware layer to every request your app serves. It no-ops outside `/admin`.

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
![grid_columns](/GritShield/docs/images/grid_columns.png)

### `read_only`

Prevents certain columns from being edited inline.

```rust
read_only = ["id", "created_at", "updated_at"]
```

**Make ALL columns read-only:**

```rust
read_only = ["all"]
```

A column you leave out of `grid_columns` is not hidden - it is unreachable. It
stays in the table, and stays out of the grid, the CSV export and inline editing.

---
## Before You Ship This

The panel generates a developer surface. Four things to decide yourself:

- **A rejected login returns `200`** with `{"status":"error"}`, not `401`. A
  client that branches on status code reads a failed login as a success. There
  is no lockout, delay or attempt counter on that path, so put a rate limiter in
  front of it or keep the panel behind your own authenticating proxy.
- **Sessions are in process memory.** The store is a global `HashMap`, so
  sessions do not survive a restart and do not work across replicas. The cookie
  is a signed `GASESSION_ID`, `Secure` only when `APP_ENV=production`.
- **No CSRF token.** `SameSite=Lax` plus the JSON content type the login endpoint
  requires means a cross-site form cannot drive it - but that is the margin, not
  a defence.
- **Schema mutation is reachable over HTTP.** `/admin/api/create-table` and
  `/admin/api/alter-table/:table_slug/add-column` let anyone who can log in
  change your tables.

---
## Inline Editing

Any editable field (not in `read_only`) can be modified directly in the grid:

1. **Click** on any editable cell
    
2. **Type** the new value
    
3. **Press Enter** to save
    

Changes are automatically persisted.

![inline_edit](/GritShield/docs/images/inline_edit.png)

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

![infinite_scroll](/GritShield/docs/images/infinite_scroll.png)

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

![jql_explorer](/GritShield/docs/images/query_explorer.png)

---

## Running the Admin Panel

2. **Start your server:**
    
    ```bash   
    cargo run OR cargo watch -w src -x run //Reloads on code changes
    ```
3. **Navigate to:** `http://localhost:8080/admin/login`
    
4. **Login** with the credentials you set in `.env`
    
5. **Explore** /admin/dashboard
    