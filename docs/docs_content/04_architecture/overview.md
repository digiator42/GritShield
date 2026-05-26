
GritShield is built on a modular, layered architecture that prioritizes security without sacrificing developer experience.

## Core Layers

### 1. Protocol Layer
Handles raw HTTP request/response parsing with strict size limits and timeout protection.

```rust
// Request parsing with 1MB limit and 5s timeout
let req = Request::parse(&mut stream).await?;
```
