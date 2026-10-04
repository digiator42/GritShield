GritShield is an **async-first, security-hardened** web framework for Rust that eliminates the majority of OWASP Top 10 vulnerabilities by design.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
gritshield = { git = "https://github.com/digiator42/gritShield" }
```

---

## Quick Start – Hello World

Create `src/main.rs`:

```rust
use gritshield::prelude::*;

#[get("/hello")]
async fn hello() -> &'static str {
    "Hello, GritShield!"
}

#[launch]
async fn main() {
    Shield::build().launch();
}
```

## With Controller

```rust
use gritshield::prelude::*;

pub struct ApiController;

#[controller("/api/v1")]
impl ApiController {
    #[get("/hello")]
    async fn hello() -> &'static str {
        "Hello, GritShield!"
    }
}
```

Run with `cargo run` and open `http://localhost:8080/hello` / `http://localhost:8080/api/v1/hello`.

## Documentation

The full documentation is available [here](https://digiator42.github.io/GritShield/).

## Runnable Examples

Each guide is a standalone crate that prints what it is doing as it starts.

| Example | Port | Needs | What it covers |
|---------|------|-------|----------------|
| [security](examples/security) | 8080 | nothing | XSS sanitization, CSRF, rate limiting, IP blacklisting |
| [routing](examples/routing) | 8081 | nothing | Controllers, params, middleware, custom 404/405 |
| [admin_panel](examples/admin_panel) | 8082 | nothing | Generated CRUD UI, admin login, audit log, CSV export |
| [openapi_swagger](examples/openapi_swagger) | 8083 | nothing | Generated OpenAPI spec and Swagger UI from your routes |
| [basic_crud](examples/basic_crud) | 8080 | Redis | Repository layer, query DSL, dependency injection |
| [mcp_server](examples/mcp_server) | 8080 | nothing | Exposing functions as AI agent tools |

The first four use separate ports and can run at the same time; the last two also
bind 8080, so stop whichever is already there.

```bash
cargo run --manifest-path examples/routing/Cargo.toml
```

Each guide's `README.md` starts with what it demonstrates and ends with what it
does *not* do yet, which is usually the more useful half.


## Quick Features Brief

* 🔒 **Security-First** – Built-in XSS, CSRF, security headers, rate limiting, and IP blacklisting.

* 🏗️ **Spring Boot‑Like DI** – Compile‑time dependency injection with zero runtime overhead. Auto‑wire your components with `#[derive(GritComponent)]`.

* ⚡ **Declarative AOP & Interceptors (`#[intercept]`)** – Wrap service methods with reusable cross-cutting concerns (audit logging, security timing, metrics).

* 📊 **Auto Admin Panel** – Full CRUD admin UI with zero frontend code. Just annotate your repository with `#[derive(GritAdmin)]` and get a complete admin interface.

* 🔍 **JQL Query Explorer** – Run SQL‑like JOIN queries directly from the browser. Supports SELECT, FROM, JOIN, and WHERE clauses.

* 📝 **OpenAPI/Swagger** – Auto‑generated API documentation from your schemas. Access at `/admin/docs`.

* 🔐 **RBAC + Capabilities** – Fine‑grained role-based and capability-based access control with compile‑time verification.

* 🧩 **Compile‑Time Macros** – All the magic happens at compile time. Zero runtime reflection, maximum performance.

* 🤖 **Native MCP Server** – Turn any function into an AI agent tool with `#[mcp_tool]`, `#[mcp_resource]` and `#[mcp_prompt]`. Ships SSE, streamable HTTP and stdio transports, a per‑tool kill switch, and a capability manager at `/admin/mcp`.


## License

Apache‑2.0 
