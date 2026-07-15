 Dependency Injection (IoC) System

GritShield provides **compile-time dependency injection** with zero runtime overhead, inspired by Spring Boot's `@Autowired`.

The Two Approaches

## `#[component]` 

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

## `GritComponent`

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

## `provide!`

For explicit registration of dependencies (config values, API keys, etc.):

```rust

provide!(PaymentService, PaymentService::new("sk_live_123".to_string()));

provide!(AppConfig, AppConfig {
    max_connections: 100,
    timeout_seconds: 30,
});

```


## Verification at Boot

```rust

// At application startup, verify all dependencies are registered, 
// this is by default called at routes intialization, you don't have to call it, 
AutoWire::boot_di_container();
```