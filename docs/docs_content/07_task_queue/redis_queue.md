Queueing background jobs through Redis offers several distinct advantages, especially compared to in-memory queues or relational databases like PostgreSQL:

### Adding Redis Dependency

First, ensure `redis` and `tokio` are in your `Cargo.toml`:

Ini, TOML

```
[dependencies]
redis = { version = "0.24", features = ["tokio-comp"] }
tokio = { version = "1", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
```

### Step 1: Implementing `RedisJobQueue` (`JobStorage`)

Here is how `RedisJobQueue` implements the `JobStorage` trait. It uses Redis Lists (`RPUSH` / `LPOP`) to store JSON-serialized `JobEnvelope` structs.

Rust

```rust
use async_trait::async_trait;
use redis::AsyncCommands;
use std::fmt::Debug;
use gritshield::core::event_bus::{JobEnvelope, JobStorage};

#[derive(Clone)]
pub struct RedisJobQueue {
    client: redis::Client,
    queue_name: String,
}

impl RedisJobQueue {
    pub fn new(client: redis::Client, queue_name: impl Into<String>) -> Self {
        Self {
            client,
            queue_name: queue_name.into(),
        }
    }
}

impl Debug for RedisJobQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RedisJobQueue")
            .field("queue_name", &self.queue_name)
            .finish()
    }
}

#[async_trait]
impl JobStorage for RedisJobQueue {
    async fn enqueue(&self, job: JobEnvelope) -> Result<(), String> {
        let mut con = self
            .client
            .get_async_connection()
            .await
            .map_err(|e| format!("Redis connection error: {}", e))?;

        // Serialize job envelope into JSON string for Redis storage
        let payload = serde_json::to_string(&job)
            .map_err(|e| format!("Serialization error: {}", e))?;

        // Push payload to the back of the Redis list
        con.rpush::<_, _, ()>(&self.queue_name, payload)
            .await
            .map_err(|e| format!("Redis RPUSH error: {}", e))?;

        Ok(())
    }

    async fn fetch_next(&self) -> Result<Option<JobEnvelope>, String> {
        let mut con = self
            .client
            .get_async_connection()
            .await
            .map_err(|e| format!("Redis connection error: {}", e))?;

        // Pop payload from the front of the Redis list
        let raw_job: Option<String> = con
            .lpop(&self.queue_name, None)
            .await
            .map_err(|e| format!("Redis LPOP error: {}", e))?;

        if let Some(json) = raw_job {
            let job: JobEnvelope = serde_json::from_str(&json)
                .map_err(|e| format!("Deserialization error: {}", e))?;

            // Check if scheduled delay execution time has arrived
            let now = chrono::Utc::now().timestamp();
            if job.run_at <= now {
                Ok(Some(job))
            } else {
                // Re-queue back to Redis if scheduled for the future
                self.enqueue(job).await?;
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    async fn complete(&self, _job_id: &str) -> Result<(), String> {
        // Popped directly out of Redis upon fetch; no cleanup needed
        Ok(())
    }

    async fn fail(&self, job: JobEnvelope, error: &str) -> Result<(), String> {
        eprintln!("[REDIS JOB RETRY] Job ID: {} | Error: {}", job.id, error);
        // Re-enqueue job back to Redis for retry backoff
        self.enqueue(job).await
    }
}
```

### Step 2: Define Your Job & Controller

Write the job struct and HTTP controller without needing to know anything about Redis.

Rust

```rust
use std::time::Duration;
use gritshield::prelude::*;

// 1. Define the Job Data Layout & Execution Logic
#[derive(Serialize, Deserialize, GritJob)]
pub struct SendEmailJob {
    pub recipient: String,
    pub template_id: String,
}

#[job(name = "send_email_job", retries = 3)]
impl SendEmailJob {
    pub async fn perform(&self) -> Result<(), String> {
        println!(
            " [WORKER] Processing SendEmailJob via Redis! Sending '{}' to {}",
            self.template_id, self.recipient
        );
        Ok(())
    }
}

// 2. Define the Controller
pub struct EmailController;

#[controller("/api/email")]
impl EmailController {
    #[post("/send")]
    pub async fn trigger_email(ctx: RequestContext) -> Response {
        let job = SendEmailJob {
            recipient: "user_1@gritshield.io".to_string(),
            template_id: "welcome_onboarding".to_string(),
        };

        // call .enqueue_in using ctx.job_queue
        if let Some(queue) = &ctx.job_queue {
            match job.enqueue_in(queue, Duration::from_secs(5)).await {
                Ok(job_id) => {
                    println!(" [CONTROLLER] Enqueued job ID {} to Redis!", job_id);
                }
                Err(e) => eprintln!("Failed to enqueue: {}", e),
            }
        }

        Response::ok("Email job enqueued to Redis background queue!")
    }
}
```

### Step 3: Bootstrapping in `main.rs`

```rust
let redis_client = redis::Client::open("redis://127.0.0.1:6379/")?;
let router = Router::new().with_job_queue(Arc::new(RedisJobQueue::new(redis_client, "grit_jobs")), 10);
```