# Getting Started with GritShield

Welcome to GritShield! This guide will help you build your first secure web application.

## Prerequisites

- Rust 1.70 or later
- Basic knowledge of async Rust

## Quick Start

Create a new Rust project:

```bash
cargo new myapp
cd myapp
```

Add GritShield to Cargo.toml:

```toml
[dependencies]
gritshield = { version = "0.1.1" }
tokio = { version = "1", features = ["full"] }
```

Create your first handler in src/main.rs:

```rust
use gritshield::prelude::*;

#[get("/")]
async fn hello(ctx: RequestContext) -> &'static str {
    "Hello, GritShield!"
}

#[tokio::main]
async fn main() {
    let router = Router::new().mount_logger();
    ignite("127.0.0.1", "8080", router, false).await;
}
```

Run it:

```bash
cargo run
```

Visit http://localhost:8080 to see your app!