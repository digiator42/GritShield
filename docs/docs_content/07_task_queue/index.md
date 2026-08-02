# Event Bus & Async Job Queue

## Architecture

```bash
            +-------------------------------------------------+
            |   #[derive(GritEvent)] / #[derive(GritJob)      |
            +--------------+----------------------------------+
                                     |
                                     v
                                     |
                                     v
 +------------------+  Event Publish |  Enqueue Job   +-------------------+
 |  HTTP Handler /  |<---------------+--------------->| Job Storage Engine|
 | Controller Route |                |                | (Memory/Pg/SQLite)|
 +------------------+                v                +---------+---------+
                            +--------+-------+                  |
                            |  Async Event   |                  v
                            | Dispatch Bus   |        +---------+---------+
                            +----------------+        | Background Worker |
                                                      |   Pool / Retry    |
                                                      +-------------------+
```


## Event Bus (Pub/Sub)

Use the Event Bus when an action in your app should trigger multiple side effects without cluttering your core controller or domain logic.

### Defining & Publishing an Event

Rust

```rust
use gritshield::prelude::*;

// Define the Event struct
#[derive(Clone, GritEvent)]
pub struct UserRegisteredEvent {
    pub user_id: String,
    pub email: String,
}

pub struct WelcomeEmailHandler;

#[event]
impl WelcomeEmailHandler {
    pub async fn handle(&self, event: Arc<UserRegistered>) {
        println!("Sending email to: {}", event.email);
    }
}

// Publish it inside your controller
#[controller("/api/users")]
impl UserController {
    #[post("/register")]
    pub async fn register(ctx: RequestContext) -> Response {
        let new_user = db::create_user().await;

        // Publish event to the bus — fire & forget!
        ctx.event_bus.publish(UserRegisteredEvent {
            user_id: new_user.id,
            email: new_user.email,
        });

        Response::ok("User created successfully!")
    }
}
```

### Subscribing to an Event

Rust

```rust
// Register a listener anywhere in your service code
ctx.event_bus.register_fn(|event: Arc<UserRegisteredEvent>| async move {
    println!("Analytics listener logged new registration for: {}", event.email);
});
```

## Job Queue (Background Tasks)

Use the Job Queue when you have heavy, slow, or network-bound tasks that shouldn't block the HTTP response and require resilience (retries, delay schedules, persistent storage).

### Defining a Job

Rust

```rust
use gritshield::prelude::*;

#[derive(Serialize, Deserialize, GritJob)]
pub struct CompressImageJob {
    pub image_url: String,
    pub target_resolution: String,
}

#[job(name = "compress_image", retries = 5)]
impl CompressImageJob {
    pub async fn perform(&self) -> Result<(), String> {
        // Heavy processing logic here
        image_service::resize(&self.image_url, &self.target_resolution).await?;
        Ok(())
    }
}
```

### Queueing a Job in a Controller

Rust

```rust
#[post("/upload")]
pub async fn upload_avatar(ctx: RequestContext) -> Response {
    let job = CompressImageJob {
        image_url: "https://storage.s3.com/avatar.png".into(),
        target_resolution: "250x250".into(),
    };

    // Run immediately in background...
    let _ = job.enqueue(queue).await;

    // ...or schedule with a delay!
    let _ = job.enqueue_in(queue, Duration::from_secs(30)).await;


    Response::ok("Image uploaded! Compression queued.")
}
```

### Cron Job

You could run a job repeatedly using cron field (which internally uses cron crate), pretty much like the cron in linux but with more fields

```
 sec   min   hour   day_of_month   month   day_of_week   year

  |     |     |          |           |          |         |
  *     *     *          *           *          *         *
```

This cron job runs each 5 secs infinitely.

```rust
#[derive(Serialize, Deserialize, Default)]
pub struct PulseCheckJob;

#[job(name = "pulse_check", cron = "0/5 * * * * *", retries = 1)]
impl PulseCheckJob {
    pub async fn perform(&self) -> Result<(), String> {
        println!("⏱️ [CRON PULSE] Executed cron job every minute!");
        Ok(())
    }
}
```

## EventBus vs Job Queue: When to Use Which?

| **Feature**        | **EventBus (GritEvent)**                    | **JobQueue (GritJob)**                          |
| ------------------ | ------------------------------------------- | ----------------------------------------------- |
| **Execution**      | Immediate / In-Memory Broadcast             | Deferred / Polled Queue                         |
| **Persistence**    | None (Ephemeral in-process)                 | Database / Memory / Redis                       |
| **Retries**        | No built-in retry                           | Exponential Backoff Retries                     |
| **Scheduling**     | Runs right now                              | Supports delayed scheduling (`run_at`)          |
| **Primary Target** | Internal app lifecycle, WebSockets, Metrics | Billing, Email Sending, PDF Exports, Migrations |

