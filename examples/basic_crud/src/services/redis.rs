use gritshield::component;
use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::sync::OnceCell;

#[derive(Clone)]
pub struct RedisService {
    client: redis::Client,
    // We use a thread-safe OnceCell to initialize the connection manager lazily
    manager: Arc<OnceCell<ConnectionManager>>,
}

impl RedisService {
    /// Create a new Redis client instance. This is INSTANT and does NOT make network calls.
    pub fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        let client = redis::Client::open(redis_url)?;
        Ok(Self {
            client,
            manager: Arc::new(OnceCell::new()),
        })
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
