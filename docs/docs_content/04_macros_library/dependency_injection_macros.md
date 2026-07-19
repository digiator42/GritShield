Dependency Injection (IoC) System

GritShield provides **compile-time dependency injection** with zero runtime overhead, inspired by Spring Boot's `@Autowired`.

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

`GritComponent` works exactly like #[component], but with struct fields, in some cases you might want to inject self dependencies instead of multiple method args, as below example.

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

### Explicit Registration

When registering components that can not be annotated with `#[derive(GritComponent)]` (such as raw third-party clients, or environment configuration parameters), the runtime injection capability is split into two straightforward parts:

#### 1. `mark_injectable!` (Module Scope)

To comply with Rust's strict local definition traits rules and eliminate compilation warnings, you must authorize a type for dynamic injection at the **module level root scope** (outside of function boundaries):

Rust

```rust
// at your file or module root level scope
mark_injectable!(RedisService);
mark_injectable!(AppConfig);
```

#### 2. `inject!` (Execution Scope)

Once a type is explicitly marked, instantiate it and store it directly inside the active global runtime environment using `inject!`. This can safely happen inside initialization blocks or async setup routines:

Rust

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

### Verification at Boot

To ensure missing references fail fast before serving traffic, ignite and evaluate the dynamic dependency tree inside your bootstrap sequence:

Rust

```rust
#[tokio::main]
async fn main() {
    // 1. Run dynamic container setup
    auto_wire().await;

    // 2. Validate structural graph completeness
    AutoWire::boot_di_container();
}
```

## **Paradigm B:**

**Strict Compile-Time Safe** (The Rust Way)

### Define Components

Your controller structure remains exactly the same! But for compile-time wiring you need to annotate the dependency with `#[derive(GritWire)]` macro.

Rust

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
    pub ps: Arc<PaymentService>,
}

impl OrderController {
    pub async fn checkout(&self, ctx: RequestContext) -> Response {
        Response::ok("Compile-time safety verified!".to_string())
    }
}
```

### `WireContainer`

You declare a concrete container struct holding your top-level dependencies. Add the `#[derive(WireContainer)]` macro to automatically compile the trait-bound structural proofs.

Rust

```rust
use gritshield::core::ioc::WireContainer;

#[derive(Clone, WireContainer)]
pub struct AppContainer {
    pub db: Arc<DatabasePool>,
    pub ps: Arc<PaymentService>,
}
```

### Mount & Ignite

Manually assemble your structural graph. Use `.compile_time_wire(&container)` to generate an immutable, thread-safe controller clone instance. Then pass it cleanly into your declarative `Router` using scoped futures.

Rust

```rust
use gritshield::routing::engine::{Router, HttpMethod};
use gritshield::deps::futures::future::FutureExt;

#[tokio::main]
async fn main() {
    // Explicitly build the typed container
    let container = AppContainer {
        db: Arc::new(DatabasePool),
        ps: Arc::new(PaymentService),
    };

    // Safely wire the controller.
    // This will FAIL to compile if AppContainer misses `db` or `ps`!
    let order_controller = OrderController::compile_time_wire(&container);

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

## What Happens When a Dependency is Missing?

### In Paradigm A (Dynamic):

If you forget to provide `PaymentService`, your application will compile successfully, but it will safely panic right at boot time during verification before processing incoming connections:

- To make this safe, you need to call boot_di_container at bootstrap.

Plaintext

```shell
thread 'main' panicked at 'GritShield DI graph is incomplete (1 missing dependency):
- OrderController requires PaymentService but it was not provided!'
```

### In Paradigm B (Compile-Time):

If you remove `ps: Arc<PaymentService>` from `AppContainer`, **your code will refuse to compile entirely**. The compiler checks the generic bounds on `compile_time_wire` and throws a clear error message:

Plaintext

```
error[E0277]: the trait bound `AppContainer: HasComponent<PaymentService>` is not satisfied
  --> src/main.rs:24:28
   |
24 |     let order_controller = OrderController::compile_time_wire(&container);
   |                            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ the trait `HasComponent<PaymentService>` is not implemented for `AppContainer`
```
