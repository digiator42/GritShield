# Admin panel - GritShield developer guide

A CRUD admin panel generated from your models, with login, an audit trail and CSV
export. No HTML templates, no route wiring, no `mount_admin()`.

```
src/main.rs               database, seeding, and the one required call: mount_db
src/models/product.rs     a SeaORM entity plus GritModel and GritRelation
src/repositories/product.rs   GritAdmin: grid columns, edit rules, /admin routes
```

## Run it

```bash
# Log in with credentials you choose - see "Credentials" below.
GRITSHIELD_ADMIN_USER=admin GRITSHIELD_ADMIN_PASSWORD=choose-something \
  cargo run --manifest-path examples/admin_panel/Cargo.toml

# GritShield listening on http://127.0.0.1:8082
```

Port **8082**, so the security (8080) and routing (8081) guides can stay up.

Then open <http://127.0.0.1:8082/admin/login>. With no credentials set, the
password is generated and printed to stdout on startup.

The `products` grid is the thing to click through: edit a cell, delete a row,
export to CSV, then read `/admin/auditlog` and see both operations recorded with
their before/after values.

---

## What the `admin` feature actually does

Enabling the feature *is* the wiring. `Router::new()` scans the model registry
that the derives populate at startup and mounts, for each model:

| Method   | Path                          | Does                            |
|----------|-------------------------------|---------------------------------|
| `GET`    | `/admin/{model}`              | grid, filters, pager            |
| `GET`    | `/admin/{model}/search`       | cross-model search palette      |
| `GET`    | `/admin/{model}/query-explorer` | ad-hoc filter builder         |
| `GET`    | `/admin/{model}/:id`          | single record                   |
| `PATCH`  | `/admin/{model}/update-cell`  | inline cell edit                |
| `DELETE` | `/admin/{model}/delete`       | delete one                      |
| `POST`   | `/admin/{model}/bulk-delete`  | delete many                     |
| `POST`   | `/admin/{model}/bulk-create`  | insert many                     |
| `GET`    | `/admin/{model}/export`       | CSV of the current filter       |

Plus the static surface: `/admin/dashboard`, `/admin/login`, `/admin/metrics`,
`/admin/auditlog` (the framework's own model), `/admin/rbac-matrix`,
`/admin/settings/security`, `/admin/api/search-palette`,
`/admin/api/create-table`, `/admin/api/alter-table/:table_slug/add-column`.

Those handlers are registered on the **global** middleware stack, so enabling
`admin` adds one middleware layer to every request your app serves. It checks
`path.starts_with("/admin")` and returns immediately otherwise, so the rest of
the app is untouched - the example registers `/health` to demonstrate that.

---

## Three conventions the derives impose

These are the parts that cost time if you guess wrong. Each one fails with an
error that points at the derive rather than at the thing you have to change.

**1. Module layout is part of the API.** Neither derive is configurable by
default; both derive the paths they need from your names:

```text
GritModel  on models/product.rs        -> crate::repositories::product::ProductRepository
GritAdmin  on repositories/product.rs  -> crate::models::product
```

So the table `products` maps to module `product`, and `ProductRepository` must
live in `repositories/product.rs`. Move the entity to `entities/` and you get
`cannot find 'repositories' in the crate root`, attributed to `#[derive(GritModel)]`.

Both derives accept a string override - `#[grit(repository = "...")]` and
`#[repository(entity = "...")]` - and the value must be a **string literal**.
`#[repository(entity = crate::models)]` does not compile: the attribute list is
parsed as `syn::Meta`, whose name-value form only accepts a literal, so you get
`error: expected identifier` aimed at the attribute.

**2. The repository has exactly one field, and it is `db`.** Every generated
handler reconstructs the repository as `ProductRepository { db: (*db).clone() }`,
so a second required field has nowhere to come from.

```rust
#[derive(Clone, GritAdmin)]
#[repository(
    searchable = ["sku", "name"],
    grid_columns = ["id", "sku", "name", "price_cents", "created_at"],
    read_only = ["created_at"],
)]
pub struct ProductRepository {
    pub db: DatabaseConnection,
}
```

The attributes mean what they look like:

- `grid_columns` is the whole visible surface. `internal_notes` is in the table,
  filled by the seed data, and simply never rendered - a column you omit is not
  a hidden column, it is an unreachable one.
- `read_only` rejects edits with `400`. `id` is always read-only.
- Leaving `grid_columns` out falls back to `id` plus everything searchable.

**3. Doc comments go above the attribute, never inside it.**

```rust
// fine
/// Also fine.
#[repository(grid_columns = ["id"])]
```

A `///` line between two attribute arguments is a syntax error, and the span
points at the comment rather than at the attribute that cannot parse.

---

## Wiring

```rust
let db = DbManager::connect(DbConfig::default()).await?;

ensure_internal_audit_log_table(&db).await?;   // before any mutation, not optional

let router = Router::new()
    .mount_logger(LogLevel::Info)
    .mount_db(db);
```

- `mount_db` is the one call you cannot skip. Without it `ctx.db` is `None` and
  every panel handler panics on `.expect("DB connection missing")`.
- `DbConfig::default()` reads `DATABASE_URL`, falling back to `sqlite::memory:`
  with a warning - which is why this example needs no configuration. An
  in-memory database is empty on every start, hence the schema and seed in
  `main.rs`.
- `ensure_internal_audit_log_table` creates the table the panel writes every edit
  and delete into. Skip it and the grid looks fine right up until the first save,
  which then fails.

---

## Credentials

`GRITSHIELD_ADMIN_USER` and `GRITSHIELD_ADMIN_PASSWORD`. Set both.

If either is missing, a random 12-character password is generated instead. In
development it is printed to stdout; in production (`APP_ENV=production`) it is
**not** printed, so a production process started without the environment
variables is unreachable and has to be restarted to recover.

---

## Verified behaviour

Checked against a running instance with the four seeded rows:

```bash
curl -i localhost:8082/health                       # 200, outside the admin gate
curl -i localhost:8082/admin/dashboard              # 303 -> /admin/login
curl -i localhost:8082/admin/product                # 200, 4 rows, no internal_notes
curl -i 'localhost:8082/admin/product/search?q=WSHR' # 200, one hit
curl -i localhost:8082/admin/product/export         # 200, text/csv
```

Login, as the page does it - a JSON POST, `202` on success:

```bash
curl -i -X POST localhost:8082/admin/api/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"choose-something"}'
```

An inline edit, which is a form POST behind HTMX, not JSON:

```bash
curl -i -X PATCH localhost:8082/admin/product/update-cell \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -d 'id=1&column=name&name=Hex bolt M8 (rev B)'    # 200, echoes the input

curl -i -X PATCH localhost:8082/admin/product/update-cell \
  -H 'Content-Type: application/x-www-form-urlencoded' \
  -d 'id=1&column=created_at&created_at=2000-01-01'  # 400, read-only
```

Both land in `/admin/auditlog` with their before and after values.

---

## Before you ship this

The panel is a development-surface generator, not a finished product. Four
things to decide for yourself:

- **Failed logins return `200` with `{"status":"error"}`**, not `401`. A client
  that branches on status code will read a rejected login as a success. There is
  no lockout, delay or attempt counter on this path, so it is brute-forceable on
  its own - put a rate limiter in front of it, or place the panel behind your own
  authenticating proxy.
- **Sessions live in process memory.** The session store is a global `HashMap`,
  so sessions do not survive a restart and do not work across replicas. The
  cookie itself is a signed `GASESSION_ID`, `Secure` only when
  `APP_ENV=production` and `SameSite=Lax`.
- **There is no CSRF token.** `SameSite=Lax` plus the JSON content type the login
  endpoint requires means a cross-site form cannot drive it, but that is the
  margin, not a defence. State-changing admin routes are ordinary `PATCH`/`POST`
  form posts.
- **No TLS, no RBAC by default.** The example binds `127.0.0.1`. One credential
  pair guards everything; per-role access is the `rbac` feature's job, not this
  one's.

The panel also exposes `/admin/api/create-table` and
`/admin/api/alter-table/:table_slug/add-column` - schema mutation over HTTP,
reachable by anyone who can log in.