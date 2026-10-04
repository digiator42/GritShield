Dependency Injection (IoC) System

GritShield ships two DI engines that share no machinery. **Paradigm A** discovers
and builds a container at boot, the way Spring's `@Autowired` does, and costs one
`TypeId` lookup per injected value. **Paradigm B** resolves everything in `main`,
has no runtime cost at all, and turns a missing dependency into a compiler error.
Pick one, or run both in the same binary - `examples/dependency_injection` does.

## **Paradigm A:**

**Dynamic / Inventory Magic** (The Spring Boot Way)

### `#[component]` 

Constructor-Based Injection

```rust

pub struct PaymentService {
    api_key: String,
}

#[component]
impl PaymentService {
    pub fn new() -> Self {
        Self {
            api_key: "sk_live_...".to_string(),
        }
    }

    pub async fn process_payment(&self, amount: f64) -> Result<PaymentResult> {
        // Business logic
    }
}
```

But here, since PaymentService relies on DatabasePool we need to register DatabasePool as a dependency as well, otherwise di container would panic at boot.

```rust

pub struct DatabasePool;

#[component]
impl DatabasePool {
    pub fn new() -> Self {
        DatabasePool {}
    }

    pub async fn execute(&self, str: &str) {
        println!("Executing...");
    }
}

pub struct PaymentService {
    api_key: String,
    db: Arc<DatabasePool>,
}

#[component]
impl PaymentService {
    pub fn new(db: Arc<DatabasePool>) -> Self {
        Self {
            api_key: "sk_live_...".to_string(),
            db,
        }
    }

    pub async fn process_payment(&self, amount: f64) -> Result<PaymentResult> {
        // Business logic
    }
}
```

### `GritComponent`

Field-Based Injection

`GritComponent` registers exactly the same kind of `CONTEXT` entry as
`#[component]`, but reads the dependencies off struct fields instead of a
constructor. Reach for it when the type is mostly a bag of collaborators and a
`new` would be nothing but a field list.

```rust

#[derive(GritComponent)]
pub struct OrderController {
    pub db: Arc<DatabasePool>,   // Needs to be annotated as well
    pub ps: Arc<PaymentService>, // Needs to be annotated as well
    pub config: Arc<AppConfig>,  // Needs to be annotated as well
}

#[controller("/api/orders")]
impl OrderController {
    // The only change here is `self`, you can access
    #[get("/")]
    pub async fn list_orders(&self, ctx: RequestContext) -> Response {
        self.db.execute("SELECT * FROM orders").await;
        Response::ok("Orders listed")
    }

    // Or inject directly into handler methods!
    #[post("/checkout")]
    pub async fn checkout(
        ctx: RequestContext,
        payment_service: Arc<PaymentService>, // Auto-injected!
    ) -> Response {
        let amount = ctx.json::<CheckoutRequest>().await?.amount;
        payment_service.process_payment(amount).await?;
        Response::ok("Checkout complete")
    }
}
```

### Skipping Dependency Injection

In some cases, you may want to bypass the container check for specific fields that are managed by other services or third-party libraries. You can use the `#[grit(skip)]` attribute to ignore fields during dependency injection validation at boot time.

**⚠️ Best Practice:** Only use `#[grit(skip)]` for component fields that are explicitly managed by external services. Using it inappropriately can lead to runtime errors if the skipped field is not properly initialized.

```rust
#[derive(Clone, GritComponent)]
pub struct RedisService {
    #[grit(skip)]
    client: redis::Client,
    #[grit(skip)]
    manager: Arc<OnceCell<ConnectionManager>>,
}
```

In this example, the `client` and `manager` fields are managed by the Redis service itself and don't need to be injected through the DI container. The `#[grit(skip)]` attribute keeps them out of the container's graph, so they neither appear as dependencies nor have to be resolvable.

**⚠️ Sharp edge:** a skipped field is not *left alone*, it is **zero-initialized**. The derive expands `#[grit(skip)]` to `unsafe { std::mem::zeroed() }`, which is only correct when the zero value already means "nothing here" - an `Option<Client>`, or a `MaybeUninit` wrapper you fill in yourself right after `wire()`. `redis::Client` has no such zero value, so the type above relies on being overwritten before first use. For any other non-`Option` field, `#[grit(skip)]` is the wrong tool.

### Explicit Registration

When registering components that can not be annotated with `#[derive(GritComponent)]` (such as raw third-party clients, or environment configuration parameters), the runtime injection capability is split into two straightforward parts:

#### 1. `mark_injectable!` (Module Scope)

To comply with Rust's strict local definition traits rules and eliminate compilation warnings, you must authorize a type for dynamic injection at the **module level root scope** (outside of function boundaries):

```rust
// at your file or module root level scope
mark_injectable!(RedisService);
mark_injectable!(AppConfig);
```

#### 2. `inject!` (Execution Scope)

Once a type is explicitly marked, instantiate it and store it directly inside the active global runtime environment using `inject!`. This can safely happen inside initialization blocks or async setup routines:

```rust
async fn auto_wire() {
    let redis_url = "redis://127.0.0.1:6379/";
    let redis_service = RedisService::new(redis_url).unwrap();

    // Safely submit the constructed instance into the container pool
    inject!(RedisService, redis_service);

    inject!(AppConfig, AppConfig {
        max_connections: 100,
        timeout_seconds: 30,
    });
}
```

`inject!` must run **before** `Router::new()`, and not for the reason you would expect. `inventory::submit!` expands to a process constructor, so the *graph entry* for `AppConfig` is published before `main` even starts, and the graph check inside `Router::new()` would pass even with `inject!` further down the function. What is actually ordered is the *value*: the boot hooks resolve their dependencies while `Router::new()` runs. Move `inject!` below it and boot dies with

```text
Critical Bootstrap DI Fault: Failed to resolve dependency 'AppConfig'
required by component 'PaymentService'
```

So: `mark_injectable!` at module scope, `inject!` above the router, and you never have to touch `main.rs` again when a new component joins the graph.

## **Paradigm B:**

**Strict Compile-Time Safe** (The Rust Way)

Paradigm B never touches the global runtime context. It is a second, entirely
standalone way to build the same object graph, and the two can be mixed in one
binary.

### Define Components

Your controller structure remains exactly the same! But for compile-time wiring you need to annotate the dependency with `#[derive(GritWire)]` macro.

```rust
use std::sync::Arc;
use crate::GritWire;
use gritshield::routing::engine::RequestContext;
use gritshield::http::response::Response;

pub struct DatabasePool;
pub struct PaymentService;

#[derive(Clone, GritWire)]
pub struct OrderController {
    pub db: Arc<DatabasePool>,
    pub config: AppConfig,
}

impl OrderController {
    pub async fn checkout(&self, ctx: RequestContext) -> Response {
        Response::ok("Compile-time safety verified!".to_string())
    }
}
```

Note the deliberate mix: `db` is an `Arc` and is moved out of the container, while
`config` is a plain `AppConfig` and is **cloned** from the container's `Arc`. A
controller may use either shape, and does not have to be consistent.

### `WireContainer`

You declare a concrete container struct holding your top-level dependencies. Add the `#[derive(WireContainer)]` macro to automatically compile the trait-bound structural proofs.

```rust
use gritshield::core::ioc::WireContainer;

#[derive(Clone, WireContainer)]
pub struct AppContainer {
    pub db: Arc<DatabasePool>,
    pub config: Arc<AppConfig>,
}
```

**Every container field must be `Arc<T>`.** `HasComponent::get_component`
returns `Arc<T>`, and the derive binds each field straight into that return
type, so a bare `AppConfig` field fails to compile. The workaround is not a
plain field - it is to hold the `Arc` here and clone the value out in the
controller, which is exactly what the example above does.

### Mount & Ignite

Manually assemble your structural graph. Use `.wire(&container)` to generate an immutable, thread-safe controller clone instance. Then pass it cleanly into your declarative `Router` using scoped futures.

```rust
use gritshield::routing::engine::{Router, HttpMethod};
use gritshield::deps::futures::future::FutureExt;

#[tokio::main]
async fn main() {
    // Explicitly build the typed container
    let container = AppContainer {
        db: Arc::new(DatabasePool),
        config: Arc::new(AppConfig::from_env()),
    };

    // Safely wire the controller.
    // This will FAIL to compile if AppContainer misses `db` or `config`!
    let order_controller = OrderController::wire(&container);

    // Explicitly mount routes using standard clone-capture closures
    let router = Router::new()
        .route((
            "/api/orders/checkout",
            HttpMethod::GET,
            move |ctx: RequestContext| {
                let oc = order_controller.clone();
                async move { oc.checkout(ctx).await }.boxed()
            }
        ));

    // Ignite
    ignite("127.0.0.1", "8080", router).await;
}
```

The closure is the entire ownership story: `wire()` returns an `Arc`, and each
request gets a clone of it. Nothing is discovered automatically, so a new
controller is one extra `.route(...)` line - the trade for never having written
a `use` for the container.

## What Happens When a Dependency is Missing?

### In Paradigm A (Dynamic)

It depends on *where* the gap is, and only one of the three cases waits for
runtime.

**A handler parameter whose type was never made injectable** never reaches the
container - it is a compile error, because the route macro asserts
`RuntimeInjectable` for every injected parameter:

```text
error[E0277]: the trait bound `InvoiceService: RuntimeInjectable` is not satisfied
```

**A component whose dependency has no provider at all** is the case that defers
to boot. Verification happens inside `Router::new()` (you never call
`boot_di_container` yourself), before any connection is accepted, and every
missing edge is reported in one panic instead of one at a time:

```text
thread 'main' panicked at src/core/ioc.rs:169:13:
GritShield DI graph is incomplete (2 missing dependencies):
  - 'InvoiceService' requires 'DatabasePool', but nothing registered that type. Add #[component] / #[derive(GritComponent)] to it, or register it explicitly with inject!(DatabasePool, ...).
  - 'PaymentService' requires 'DatabasePool', but nothing registered that type. Add #[component] / #[derive(GritComponent)] to it, or register it explicitly with inject!(DatabasePool, ...).
```

### In Paradigm B (Compile-Time):

If you remove `db: Arc<DatabasePool>` from `AppContainer`, **your code will refuse to compile entirely**. The compiler checks the generic bounds on `wire` and throws a clear error message, with the container named as the thing that is wrong:

```text
error[E0277]: the trait bound `AppContainer: HasComponent<DatabasePool>` is not satisfied
  --> src/strict.rs:69:45
   |
69 |     let checkout = CheckoutController::wire(&container);
   |                    ^^^^^^^^^^ unsatisfied trait bound
   |
   = help: the trait `HasComponent<DatabasePool>` is not implemented for `AppContainer`
```

## Runnable Example

`examples/dependency_injection` runs both paradigms in a single binary on port
`8085` - no database, no Redis, no session - and serves the container's own
Mermaid graph at `/api/billing/graph`:

```bash
cargo run --manifest-path examples/dependency_injection/Cargo.toml
```

| Route | Shows |
|-------|-------|
| `POST /api/billing/charge` | constructor-injected service as a handler argument, plus `AppConfig` rejection |
| `GET /api/billing/invoice/:customer` | field-injected service on a `#[controller]` |
| `GET /api/billing/graph` | `AutoWire::export_mermaid()` - the graph including handler edges |
| `GET /api/strict/info` | a `#[derive(GritWire)]` controller holding a cloned config |
| `POST /api/strict/refund` | a wired controller calling a `#[component]` service |
