# Gritshield Route Macros

A set of convenient procedural macros for defining HTTP routes in **Gritshield**.

## Overview

This crate provides attribute macros (`#[get]`, `#[post]`, `#[put]`, etc.) that automatically register routes using the `gritshield::routing::trie` system.

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
gritshield = { path = "../gritshield" }
```

## Supported Macros

| Macro     | HTTP Method | Supports Role |
|-----------|-------------|---------------|
| `#[get]`  | GET         | Yes           |
| `#[post]` | POST        | Yes           |
| `#[put]`  | PUT         | Yes           |
| `#[patch]`| PATCH       | Yes           |
| `#[delete]` | DELETE    | Yes           |

## Syntax Options

```rust
// Simple path
#[get("/api/hello")]

// With role protection
#[get("/api/secure", role = "Operator")]
#[post("/api/data", role = "Admin")]
```

## How It Works

- Each macro generates a wrapper function that converts your handler into a `BoxFuture`.
- Automatically registers the route using `inventory::submit!`.
- Supports optional role-based authorization.

## Requirements

- Your handler must accept `gritshield::routing::trie::RequestContext`
- Your handler should return a type that implements `IntoResponse`

---
