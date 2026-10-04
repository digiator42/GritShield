# OpenAPI / Swagger

Automatic API documentation for the routes you already wrote. There is no
`openapi.yaml` in this example, and no route carries a documentation-only
annotation beyond `body = CreateUser`.

```bash
cargo run --manifest-path examples/openapi_swagger/Cargo.toml
```

Then open **<http://127.0.0.1:8083/admin/docs>**.

No database, no Redis, no login. The whole example is one file.

## The feature

```toml
gritshield = { path = "../../", features = ["swagger"] }
```

That is the entire setup. `Router::new()` notices the feature and mounts two
routes on top of yours:

| Route | Serves |
| --- | --- |
| `GET /admin/docs` | Swagger UI |
| `GET /admin/docs/openapi.json` | the spec, as JSON |

Both are generated per request from registries your code has already filled in:

- the route inventory written by `#[controller]`, `#[get]`, `#[post]`, `#[delete]`;
- the schema registry written by `GritSchema` and `GritModel`.

So the spec is a projection of the code rather than a second copy of it. Delete
the controller and the endpoints disappear from the documentation.

## What you get for free

Each `#[get]` / `#[post]` attribute becomes an operation, grouped under the
**Developer Routes** tag, with the path, the verb and any `:param` segments:

```rust
#[get("/users/:id")]
pub async fn get_user(ctx: RequestContext) -> Response { /* ... */ }
```

```json
{
  "/api/v1/users/{id}": {
    "get": {
      "summary": "GET /api/v1/users/{id}",
      "description": "route: /api/v1/users/{id}",
      "operationId": "dev_get__api_v1_users__id",
      "tags": ["Developer Routes"],
      "parameters": [
        {
          "name": "id",
          "in": "path",
          "required": true,
          "schema": { "type": "string" },
          "description": "id parameter"
        }
      ]
    }
  }
}
```

Path parameters are typed as strings regardless of what your handler does with
them. This example parses `id` by hand and answers `400` for a non-numeric
segment; the documentation cannot know that, so it describes the segment, not
your validation.

`GET` and `DELETE` on the same path share one path item, which is what makes
Swagger UI render them as two operations on one entry.

## Documenting a request body

Mark the payload with `GritSchema` and name it on the route:

```rust
#[derive(GritSchema, GritSanitizer, Serialize, Deserialize, Debug)]
pub struct CreateUser {
    pub email: String,
    pub name: String,
    pub nickname: Option<String>,
}

#[post("/users", body = CreateUser)]
pub async fn create_user(ctx: RequestContext) -> Response { /* ... */ }
```

`GritSchema` registers the struct under its own name at startup; `body =
CreateUser` is the route naming that registration. You get:

```json
"requestBody": {
  "required": true,
  "content": {
    "application/json": {
      "schema": {
        "type": "object",
        "properties": {
          "email": { "type": "string" },
          "name": { "type": "string" },
          "nickname": { "type": "string" }
        },
        "required": ["email", "name"]
      }
    }
  }
}
```

`Option<T>` fields land in `properties` but not in `required`. That is the only
distinction the derive draws.

The type mapping is blunt on purpose: `String` stays a string, every integer
width collapses to `i64`, `bool` stays a boolean, dates become date-time
strings, and **anything it does not recognise becomes a string**. Treat the
generated schema as a shape for a human reading the docs, not as a contract you
can validate a client against.

`GritSanitizer` is unrelated to documentation. `ctx.json::<T>()` requires
`T: GritSanitizable`, so a body you intend to deserialise needs that derive
too.

## Worth knowing before you rely on it

- **Responses are not described.** Every operation gets a `200` whose schema is
  the bare `{"type": "object"}` plus a `404`. GritShield does not read your
  return type, so it cannot tell you that `create_user` returns
  `201 Created`. Write those descriptions by hand if you need them.
- **No `$ref`, no reuse.** The body schema is inlined into each operation that
  uses it, and `components.schemas` stays empty for developer routes. Two
  endpoints sharing a payload get two copies of it. Registered *models* under
  the `admin` feature are the exception: they appear as `<table-slug>Model`.
- **Fields are not nullable in the output.** An `Option<String>` field is
  documented as `{"type": "string"}` with no `nullable` flag.
- **There are no security schemes.** The spec declares no bearer token, no
  cookie auth and no `security` requirements, even when your routes require
  them.
- **Titles and versions are placeholders.** `info.title` is `OpenApi API` and
  the version is `1.0.0`.
- **The UI loads from a CDN.** Swagger UI 5.17.14 is pulled from
  `unpkg.com`, so the docs page renders blank without internet access even
  though `/admin/docs/openapi.json` still works. Point the `<script>` tags in
  `src/core/swagger/ui.rs` at a local copy if that matters to you.
- **Any path containing `page` gains a `page` query parameter** automatically.

In short: the generator gives you an accurate map of your routing table and a
rough map of your request bodies, for free and with no annotations. It is not a
description of your API's behaviour.

## Adding `admin`

The two doc routes live under `/admin/docs`, and `AdminAuthMiddleware` gates
every `/admin*` path except the login endpoints. Enable `admin` and both
endpoints start answering `303 -> /admin/login` until you authenticate:

```bash
curl -i http://127.0.0.1:8083/admin/docs/openapi.json
# HTTP/1.1 303 See Other
# location: /admin/login
```

That is a real trap for tooling. Anything that fetches the spec
unauthenticated - a CI step, a client generator, a link in your README - gets a
redirect to an HTML login page instead of JSON, because the redirect is chosen
by whether the path starts with `/admin/api/`, and `/admin/docs` does not.

Log in first and it works:

```bash
curl -c cookies.txt -X POST http://127.0.0.1:8083/admin/api/login \
  -H 'Content-Type: application/json' \
  -d '{"username":"admin","password":"admin123"}'
curl -b cookies.txt http://127.0.0.1:8083/admin/docs/openapi.json
```

With no `GRITSHIELD_ADMIN_USER` / `GRITSHIELD_ADMIN_PASSWORD` set, the framework
prints a random password to stdout at startup.

Once `admin` is on, the spec also lists the generated per-model endpoints and
their schemas:

```
/admin/auditlog
/admin/auditlog/{id}
/admin/auditlog/update-cell
/admin/auditlog/bulk-delete

components.schemas: audit-logsModel
```

See the [admin panel example](../admin_panel/README.md) for the model side of
this.

## Try it

```bash
# Documented body, documented 201
curl -i -X POST http://127.0.0.1:8083/api/v1/users \
  -H 'Content-Type: application/json' \
  -d '{"email":"alan@example.com","name":"Alan"}'

# Missing `email`: 400 from the framework error handler
curl -i -X POST http://127.0.0.1:8083/api/v1/users \
  -H 'Content-Type: application/json' \
  -d '{"name":"Alan"}'

# The spec the UI is rendering
curl -s http://127.0.0.1:8083/admin/docs/openapi.json
```

## Files

| File | What it shows |
| --- | --- |
| `src/main.rs` | Four routes, one `GritSchema` payload, and every annotation that carries documentation weight |
| `Cargo.toml` | The `swagger` feature and the `ctor` dependency the route macros need |