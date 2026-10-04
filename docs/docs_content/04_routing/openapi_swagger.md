# OpenAPI / Swagger

Enable the `swagger` feature and GritShield documents the routes you already
have. No spec file, no annotations to keep in sync.

```toml
[dependencies]
gritshield = { path = "../../", features = ["swagger"] }
```

`Router::new()` then mounts two routes on your behalf:

| Route | Serves |
| --- | --- |
| `GET /admin/docs` | Swagger UI |
| `GET /admin/docs/openapi.json` | the spec, as JSON |

Both are generated per request from the route inventory written by
`#[controller]` / `#[get]` / `#[post]` and the schema registry written by
`GritSchema` and `GritModel`. Deleting a route removes it from the
documentation; there is no second copy of your API to maintain.

The `admin` feature is **not** required. `register_swagger_routes` is gated on
`swagger` alone.

## What is generated

Every route attribute becomes an operation under the `Developer Routes` tag.
Path parameters are rewritten from GritShield's `:id` syntax to OpenAPI's
`{id}` and marked required:

```json
"parameters": [
  {
    "name": "id",
    "in": "path",
    "required": true,
    "schema": { "type": "string" },
    "description": "id parameter"
  }
]
```

Summaries and descriptions are derived mechanically (`"GET /api/v1/users/{id}"`,
`"route: /api/v1/users/{id}"`). Any path whose text contains `page` also gets
an automatic `page` query parameter.

## Request bodies

`GritSchema` registers a struct under its own name. Reference that name on the
route with `body =`:

```rust
#[derive(GritSchema, GritSanitizer, Serialize, Deserialize)]
pub struct CreateUser {
    pub email: String,
    pub name: String,
    pub nickname: Option<String>,
}

#[post("/users", body = CreateUser)]
pub async fn create_user(ctx: RequestContext) -> Response { /* ... */ }
```

`Option<T>` fields appear in `properties` but not in `required`. Type mapping is
deliberately lossy: integers collapse to `i64`, dates become date-time strings,
and unrecognised types become strings.

`GritSanitizer` is not documentation - `ctx.json::<T>()` requires
`T: GritSanitizable`. It is listed above only because the payload needs both
derives.

## Limits worth knowing

- **Responses are not described.** Every operation gets a `200` with the bare
  schema `{"type": "object"}` plus a `404`. The generator cannot read your
  return type, so a route that actually answers `201 Created` is still
  documented as `200`.
- **Schemas are inlined, not referenced.** A body schema is copied into each
  operation that uses it. `components.schemas` stays empty for developer
  routes; only models registered under `admin` appear there, as
  `<table-slug>Model`.
- **`Option<T>` is not marked `nullable`.** It is excluded from `required` and
  that is all.
- **No security schemes.** No bearer token, no cookie auth, no `security`
  requirements, even when the routes require them.
- **`info` is a placeholder**: title `OpenApi API`, version `1.0.0`.
- **The UI loads Swagger UI from `unpkg.com`.** The docs page renders blank
  offline even though `openapi.json` is still served.

## Interaction with `admin`

`AdminAuthMiddleware` gates every `/admin*` path except the login endpoints,
and `/admin/docs` is under `/admin`. Enabling `admin` therefore moves both doc
routes behind a login:

```bash
curl -i http://127.0.0.1:8083/admin/docs/openapi.json
# HTTP/1.1 303 See Other
# location: /admin/login
```

The redirect is chosen by whether the path starts with `/admin/api/`, which
`/admin/docs` does not, so unauthenticated tooling receives an HTML login page
rather than JSON. Authenticate first:

```bash
curl -c cookies.txt -X POST http://127.0.0.1:8083/admin/api/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"admin123"}'
curl -b cookies.txt http://127.0.0.1:8083/admin/docs/openapi.json
```

With `admin` enabled the spec additionally lists the generated per-model
endpoints (`/admin/<model>`, `/admin/<model>/{id}`, `update-cell`,
`bulk-delete`) and a `<table-slug>Model` schema for each registered model.

## Runnable example

[`examples/openapi_swagger`](https://github.com/digiator42/GritShield/tree/main/examples/openapi_swagger)
- four routes, one `GritSchema` payload, no database:

```bash
cargo run --manifest-path examples/openapi_swagger/Cargo.toml
```

Then open <http://127.0.0.1:8083/admin/docs>.