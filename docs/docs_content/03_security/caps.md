# Capability RBAC

GritShield provides a dual-layer security model that combines **compile-time structural verification** with **zero-cost runtime validation**.

By decoupling endpoint security definitions from hardcoded string roles, GritShield ensures that privilege requirements are validated at compile time while remaining fully dynamic during request execution.

## Architecture Overview

Plaintext

```text
            ┌─────────────────────────────────────────────────────────┐
            │                 declare_security_caps!                  │
            │   Maps Capability Tokens -> Allowed System Roles        │
            └────────────────────────────┬────────────────────────────┘
                                         │
                        ┌────────────────┴──────────────────┐
                        ▼                                   ▼
            ┌───────────────────┐                 ┌───────────────────┐
            │ 1. COMPILE TIME   │                 │  2. RUNTIME       │
            │    Static Fence   │                 │     Validation    │
            └─────────┬─────────┘                 └─────────┬─────────┘
                      │                                     │
                      ▼                                     ▼
            Ensures capability tokens             Evaluates incoming session roles
            exist & implement trait               against allowed role set & hierarchy
```


## Defining Capability Mappings

Capabilities act as an abstraction layer between business logic endpoints and raw user roles. Use `declare_security_caps!` to define single sources of truth for your application boundaries:

This macro automatically expands into two critical systems:

- `GritSecurityCheck`: A zero-sized trait used for compile-time structural fences.
    
- `GritCapabilityRuntime`: A metadata provider that yields allowed role slices (`&'static [&'static str]`) during HTTP dispatch.

create `src/security.rs`

Rust

```rust
use gritshield::declare_security_caps;

// Define your system roles as concrete structural tokens.
// These are the types passed into the capability matrix arrays.
pub struct Admin;
pub struct Manager;
pub struct Auditor;
pub struct Operator;
pub struct Editor;
pub struct Contributor;
pub struct Viewer;

// Define capabilities tokens
pub struct ManageBilling;
pub struct DeleteUser;
pub struct ViewLogs;
```


```rust
// Declare the single source of truth for capability authorizations.
// One invocation per application; a dedicated module (src/security.rs) works
// as well as main.rs, and keeps the matrix out of your wiring.
declare_security_caps! {
    ManageBilling => [Admin, Manager, Operator],
    DeleteUser    => [Admin],
    ViewLogs      => [Admin, Manager, Auditor],
}
```

> [!IMPORTANT]
> - `#[cap(A, B)]` is **OR** logic: the session role only has to satisfy one of
>   the listed capabilities. It is also the **only** `#[cap]` attribute allowed
>   per handler - a second one fails to compile with `cannot find attribute
>   `cap` in this scope`, so put every token in a single attribute.
> - `#[cap(..)]` and `role = "..."` on the same route are independent checks,
>   both of which must pass.

create `src/controllers/billing.rs`

When building endpoints, simply bring the capability tokens into context:

Rust

```rust
use crate::security::{ViewLogs, ManageBilling}; // Import the tokens

#[controller("/api/billing")]
impl BillingController {

    #[get("/audit-logs")]
    #[cap(ViewLogs)] // Static check ensures `ViewLogs` implements GritSecurityCheck
    pub async fn get_audit_logs(&self) -> Response {
        Response::ok("Logs retrieved successfully.")
    }

    #[post("/refund")]
    #[cap(ViewLogs, ManageBilling)] // Works with multiple capability evaluations, `or Logic`
    pub async fn process_refund(&self) -> Response {
        Response::ok("Refund processed.")
    }
}
```
> [!IMPORTANT]
> - `Admin`, `Manager` and `Auditor` can check logs `/api/billing/audit-logs`, `Operator` can't.
> - `Admin`, `Manager`, `Auditor` and `Operator` can process a refund, Access is granted if the user's role satisfies any of the declared capabilities:

## Runnable example

[`examples/rbac_caps`](https://github.com/digiator42/GritShield/tree/main/examples/rbac_caps)
walks five roles across four authorization styles and prints the measured
allow/deny matrix:

```bash
cargo run --manifest-path examples/rbac_caps/Cargo.toml
```
