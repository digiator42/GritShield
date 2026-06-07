
GritShield is an **async-first, security-hardened** web framework for Rust that eliminates the majority of OWASP Top 10 vulnerabilities by design.

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
gritshield = "0.1.0"
tokio = { version = "1", features = ["full"] }
```

---

## Quick Start – Hello World

Create `src/main.rs`:

```rust
use gritshield::prelude::*;

#[get("/")]
async fn hello(_: RequestContext) -> &'static str {
    "Hello, GritShield!"
}

#[tokio::main]
async fn main() {
    let router = Router::new().mount_logger();

    run_server("127.0.0.1", "8080", router, true).await;
}
```

Run with `cargo run` and open `http://localhost:8080`.

---

## Documentation

The full documentation is available at [https://digiator42.github.io/gritshield/](https://digiator42.github.io/GritShield/).
