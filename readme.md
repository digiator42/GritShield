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

#[get("/")]
async fn hello() -> &'static str {
    "Hello, GritShield!"
}

#[launch]
async fn main() {
    Shield::build().launch();
}
```

Run with `cargo run` and open `http://localhost:8080`.

---

## Documentation

The full documentation is available [here](https://digiator42.github.io/GritShield/).


## Quick Features Brief

🔒 **Security-First** – Built-in XSS, CSRF, security headers, rate limiting, and IP blacklisting.

🏗️ **Spring Boot‑Like DI** – Compile‑time dependency injection with zero runtime overhead. Auto‑wire your components with `#[derive(GritComponent)]`.

📊 **Auto Admin Panel** – Full CRUD admin UI with zero frontend code. Just annotate your repository with `#[derive(GritAdmin)]` and get a complete admin interface.

🔍 **JQL Query Explorer** – Run SQL‑like JOIN queries directly from the browser. Supports SELECT, FROM, JOIN, and WHERE clauses.

📝 **OpenAPI/Swagger** – Auto‑generated API documentation from your schemas. Access at `/admin/docs`.

🔐 **RBAC + Capabilities** – Fine‑grained role-based and capability-based access control with compile‑time verification.

🧩 **Compile‑Time Macros** – All the magic happens at compile time. Zero runtime reflection, maximum performance.


## License

Apache‑2.0 