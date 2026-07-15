To add Redis into your crate, you can leverage dependency injection system. This allows any controller or service to access a shared, thread-safe Redis connection pool dynamically without hardcoded dependencies.

### Step 1: Add the Redis Crate

First, add the `redis` crate to your framework's `Cargo.toml`. Since GritShield is asynchronous, we'll enable the `tokio-comp` (Tokio runtime compatibility) and `connection-manager` features:

TOML

```
[dependencies]
redis = { version = "1", features = ["tokio-comp", "connection-manager"] }
```

### Step 2: Wrap Redis into a GritShield Component

Create a wrapper struct `RedisService` to encapsulate connection pool initialization and keep database commands clean and robust. Since it relies on custom runtime configuration strings (the Redis URL), we will register it manually using `AutoWire::component` or `provider!` macro.

Create `src/services/redis_service.rs` (or place it in your service directory):

Rust

```rust
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::OnceCell;

#[derive(Clone)]
pub struct RedisService {
    pub manager: ConnectionManager,
}

// Because RedisService relies on redis_url as config url, we can't use #[component]
impl RedisService {
    /// Create a new asynchronous thread-safe Redis manager
    pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let manager = ConnectionManager::new(client).await?;
        Ok(Self { manager })
    }

    /// Internal helper to lazily get or establish the connection manager
    async fn get_manager(&self) -> Result<&ConnectionManager, redis::RedisError> {
        self.manager
            .get_or_try_init(|| async { ConnectionManager::new(self.client.clone()).await })
            .await
    }

    /// Safely set a value with an optional expiration time in seconds
    pub async fn set(
        &self,
        key: &str,
        value: &str,
        expiry_secs: Option<u64>,
    ) -> Result<(), redis::RedisError> {
        // Lazily get the connection manager (will attempt connection here, on-demand)
        let manager = self.get_manager().await?;
        let mut conn = manager.clone();
        if let Some(secs) = expiry_secs {
            let () = conn.set_ex(key, value, secs).await?;
        } else {
            let () = conn.set(key, value).await?;
        }
        Ok(())
    }

    /// Retrieve a cached string value
    pub async fn get(&self, key: &str) -> Result<Option<String>, redis::RedisError> {
        let manager = self.get_manager().await?;
        let mut conn = manager.clone();
        let val: Option<String> = conn.get(key).await?;
        Ok(val)
    }
}
```

### Step 3: Instantiate and Register in Your Setup Bootstrapper

In your main setup file, initialize `RedisService` asynchronously and register it into the DI context before triggering `boot_di_container()`:

Rust

```rust
use gritshield::core::ioc::AutoWire;
use your_crate::services::RedisService;

#[tokio::main]
async fn main() {
     // Setup local environment configurations, preferred to get it from std env
    let redis_url = "redis://127.0.0.1:6379/";

	// Use provider macro to inject RedisService into DI container
    provide!(RedisService, RedisService::new(redis_url).unwrap());

	// OR

    // Instantiate your Redis service
    let redis_service = RedisService::new(redis_url).unwrap();

	// Then call AutoWire::component
    AutoWire::component(redis_service);

    // Router init ... boot_di_container is called here by default
}
```

### Step 4: Access Redis in Your Controllers

Now that `RedisService` is registered, your structural (`#[scontroller]`) and dynamic functional (`#[controller]`) routing endpoints can request it directly.

#### A. Inside a Structural Controller (`#[derive(GritComponent)]`)

GritShield's Lombok-style struct derive will inject the `RedisService` component automatically upon resolution:

Rust

```rust
use std::sync::Arc;
use gritshield::routing::trie::RequestContext;
use gritshield::protocol::response::Response;
use crate::services::RedisService;

#[derive(GritComponent)]
pub struct CacheController {
    pub redis: Arc<RedisService>, // <-- Autowired automatically!
}

#[scontroller("/api/cache")]
impl CacheController {
    #[get("/status")]
    pub async fn check_cache(&self, ctx: RequestContext) -> Response {
        // Use the injected Redis wrapper seamlessly
        match self.redis.get("system_status").await {
            Ok(Some(status)) => Response::ok(format!("System Status from Cache: {}", status)),
            Ok(None) => Response::ok("Cache empty"),
            Err(e) => Response::error(format!("Redis Error: {}", e)),
        }
    }
}
```

#### B. Inside a Dynamic Functional Controller (`#[controller]`)

Your unified dynamic route compiler will seamlessly extract `Arc<RedisService>` at runtime from your core context when a request hits the handler:

Rust

```rust
#[controller("/api/store")]
impl StoreController {
    #[post("/save")]
    pub async fn save_item(
        ctx: RequestContext,
        redis: Arc<RedisService>, // <-- Dynamically injected argument!
    ) -> Response {
        if let Err(e) = redis.set("last_stored_item", "item_data_123", Some(60)).await {
            return Response::error(format!("Caching failed: {}", e));
        }
        Response::ok("Item cached for 60 seconds!")
    }
}
```

