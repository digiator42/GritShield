To add Redis into your crate, you can leverage the dependency injection system. This allows any controller or service to access a shared, thread-safe Redis connection pool dynamically without hardcoded dependencies.

### Step 1: Add the Redis Crate

Since GritShield is asynchronous, enable the `tokio-comp` and `connection-manager` features:

```toml
[dependencies]
redis = { version = "0.24", features = ["tokio-comp", "connection-manager"] }
```

### Step 2: Wrap Redis into a GritShield Component

Create a wrapper struct `RedisService` around the async `ConnectionManager`. Since
it needs a `redis_url` from configuration, register the instance manually with
`inject!` rather than using `#[component]`.

`src/services/redis_service.rs`:

```rust
use redis::aio::ConnectionManager;
use redis::AsyncCommands;

#[derive(Clone)]
pub struct RedisService {
    manager: ConnectionManager,
}

impl RedisService {
    pub async fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        let manager = ConnectionManager::new(client).await?;
        Ok(Self { manager })
    }

    pub async fn set(&self, key: &str, value: &str, expiry_secs: Option<u64>) -> Result<(), redis::RedisError> {
        let mut conn = self.manager.clone();
        if let Some(secs) = expiry_secs {
            conn.set_ex(key, value, secs).await?;
        } else {
            conn.set(key, value).await?;
        }
        Ok(())
    }

    pub async fn get(&self, key: &str) -> Result<Option<String>, redis::RedisError> {
        let mut conn = self.manager.clone();
        conn.get(key).await
    }
}
```

`ConnectionManager` is cheap to clone and multiplexes connections, so one
instance is shared by the whole process.

### Step 3: Instantiate and Register in Your Setup Bootstrapper

`mark_injectable!` must be at module scope. `inject!` must run **before**
`Router::new()` (boot hooks resolve dependencies while the router is built).
`new()` is async, so await it first.

```rust
use gritshield::{inject, mark_injectable};
use your_crate::services::RedisService;

mark_injectable!(RedisService);

#[tokio::main]
async fn main() {
    let redis_url = "redis://127.0.0.1:6379/";
    let redis_service = RedisService::new(redis_url).await.unwrap();

    inject!(RedisService, redis_service);

    // boot_di_container() is called inside Router::new()
    let router = Router::new() /* ... */;
}
```

`inject!` stores the value in the dynamic `CONTEXT` container, where
`CONTEXT.resolve::<RedisService>()` returns `Arc<RedisService>`. Resolved
components are memoized for the process lifetime (effectively singletons).

### Step 4: Access Redis in Your Controllers

#### A. Inside a Structural Controller (`#[derive(GritComponent)]`)

The derive injects `Arc<RedisService>` for every non-skipped field:

```rust
use std::sync::Arc;
use gritshield::{controller, get, GritComponent};
use gritshield::prelude::*;
use crate::services::RedisService;

#[derive(GritComponent)]
pub struct CacheController {
    pub redis: Arc<RedisService>, // autowired on resolution
}

#[controller("/api/cache")]
impl CacheController {
    #[get("/status")]
    pub async fn check_cache(&self, _ctx: RequestContext) -> Response {
        match self.redis.get("system_status").await {
            Ok(Some(status)) => Response::ok(format!("System Status from Cache: {status}")),
            Ok(None) => Response::ok("Cache empty"),
            Err(e) => Response::internal_error(format!("Redis Error: {e}")),
        }
    }
}
```

A `#[controller]` whose methods take `&self` is resolved from `CONTEXT` on each
request, so the controller type must itself be registered (`#[derive(GritComponent)]`
does that).

#### B. Inside a Dynamic Functional Controller (`#[controller]`)

Route macros resolve any typed argument other than `RequestContext` from the
container:

```rust
use std::sync::Arc;
use gritshield::{controller, post};
use gritshield::prelude::*;
use crate::services::RedisService;

#[controller("/api/store")]
impl StoreController {
    #[post("/save")]
    pub async fn save_item(
        _ctx: RequestContext,
        redis: Arc<RedisService>, // dynamically injected argument
    ) -> Response {
        if let Err(e) = redis.set("last_stored_item", "item_data_123", Some(60)).await {
            return Response::internal_error(format!("Caching failed: {e}"));
        }
        Response::ok("Item cached for 60 seconds!")
    }
}
```

For a zero-runtime-cost alternative, use `#[derive(WireContainer)]` on an
all-`Arc` container plus `#[derive(GritWire)]` on the controllers and call
`Type::wire(&container)` in `main`.
