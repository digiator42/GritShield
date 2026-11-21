# Event Bus & Async Job Queue

## Architecture

```bash
             +-------------------------------------------------+
             |   #[derive(GritEvent, Clone)] / #[derive(GritJob)] |
             +--------------+----------------------------------+
                            |                        |
                            v                        v
              +-------------+----------+   +---------+----------+
              |  Async Event Dispatch   |   | Job Storage Engine|
              |  Bus (broadcast, 1024)  |   | (in-memory + your |
              +-------------+----------+   | Redis/PG backend) |
                            ^              +---------+----------+
       event.publish().await|                        | job.enqueue()
                            |                        v
              +-------------+----------+   +---------+----------+
              |  HTTP Handler /        |   | Background Workers|
              |  Controller Route      |   | 10 workers + cron |
              +------------------------+   +--------------------+
```

Neither `GritEvent`/`GritJob` nor their `#[event]`/`#[job]` attribute macros are
in the prelude. Import them explicitly:

```rust
use gritshield::{event, job, GritEvent, GritJob};
use serde::{Serialize, Deserialize};
```

`Router::new()` discovers every `#[event]` handler and `#[job]` registration.
`ignite()` starts the worker pool and the cron scheduler automatically; there is
nothing to mount.

## Event Bus (Pub/Sub)

Use the Event Bus when an action in your app should trigger multiple side effects
without cluttering your core controller or domain logic. Each handler gets its
own `broadcast` receiver and runs on a spawned task; all handlers for an event
type fire (fan-out).

### Defining & Publishing an Event

```rust
use std::sync::Arc;
use gritshield::{event, GritEvent};
use gritshield::prelude::*;

#[derive(Clone, GritEvent)]
pub struct UserRegisteredEvent {
    pub user_id: String,
    pub email: String,
}

pub struct WelcomeEmailHandler;

#[event]
impl WelcomeEmailHandler {
    pub async fn handle(&self, event: Arc<UserRegisteredEvent>) {
        println!("Sending email to: {}", event.email);
    }
}

#[controller("/api/users")]
impl UserController {
    #[post("/register")]
    pub async fn register(ctx: RequestContext) -> Response {
        // Fire-and-forget on the bus (synchronous, ignores send errors):
        ctx.event_bus.publish(UserRegisteredEvent {
            user_id: "42".into(),
            email: "ada@example.com".into(),
        });

        Response::ok("User created successfully!")
    }
}
```

Inside a transaction, prefer the derive-generated `event.publish().await`: it
stages the event and only dispatches it after a successful commit. That method
requires `Clone` (the blanket `GritEventExt` impl does).

### Subscribing with a closure

```rust
ctx.event_bus.register_fn(|event: Arc<UserRegisteredEvent>| async move {
    println!("Analytics listener logged new registration for: {}", event.email);
});
```

## Job Queue (Background Tasks)

Use the Job Queue for heavy, slow, or network-bound tasks that shouldn't block
the HTTP response and need retries or delayed execution.

### Defining a Job

Both macros are required: `#[derive(GritJob)]` plus `#[job]` on an inherent impl
with `async fn perform(&self)`. The derive alone only adds a type-name helper;
`#[job]` generates the `GritJob` trait impl and the compile-time registration.

```rust
use gritshield::{job, GritJob};
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, GritJob)]
pub struct CompressImageJob {
    pub image_url: String,
    pub target_resolution: String,
}

#[job(name = "compress_image", retries = 5)]
impl CompressImageJob {
    pub async fn perform(&self) -> Result<(), String> {
        // image_service::resize(&self.image_url, &self.target_resolution).await?;
        Ok(())
    }
}
```

### Queueing a Job in a Controller

`#[job]` generates inherent `enqueue()` and `enqueue_in(delay)` methods. Both
return `Result<String, String>` (the job id). Inside an HTTP handler the active
queue is a task-local installed from `router.job_queue`, so no queue argument is
needed. For an explicit backend use `GritJobExt::{enqueue_immediately,
enqueue_directly}` (`use gritshield::GritJobExt;`).

```rust
use std::time::Duration;

#[post("/upload")]
pub async fn upload_avatar(ctx: RequestContext) -> Response {
    let job = CompressImageJob {
        image_url: "https://storage.s3.com/avatar.png".into(),
        target_resolution: "250x250".into(),
    };

    // Run immediately in the background...
    match job.enqueue().await {
        Ok(id) => println!("enqueued {id}"),
        Err(e) => eprintln!("enqueue failed: {e}"),
    }

    // ...or schedule with a delay.
    let _ = job.enqueue_in(Duration::from_secs(30)).await;

    Response::ok("Image uploaded! Compression queued.")
}
```

Inside `#[transactional]`, `.enqueue()` and `.enqueue_in()` stage the job and
flush it post-commit; a rollback discards it.

### Cron jobs

Cron is 6 fields (`sec min hour day_of_month month day_of_week`), validated at
compile time. Cron jobs always receive a `null` payload, so they must be
fieldless unit structs.

```rust
#[derive(Serialize, Deserialize, Default, GritJob)]
pub struct PulseCheckJob;

#[job(name = "pulse_check", cron = "0/5 * * * * *", retries = 1)]
impl PulseCheckJob {
    pub async fn perform(&self) -> Result<(), String> {
        println!("[CRON PULSE] executed every 5 seconds");
        Ok(())
    }
}
```

## EventBus vs Job Queue: When to Use Which?

| **Feature**        | **EventBus (GritEvent)**                    | **JobQueue (GritJob)**                          |
| ------------------ | ------------------------------------------- | ----------------------------------------------- |
| **Execution**      | Immediate in-process broadcast              | Deferred / polled queue                         |
| **Persistence**    | None (ephemeral, capacity 1024)             | In-memory by default; implement `JobStorage` for Redis/Postgres |
| **Retries**        | No built-in retry                           | Exponential backoff (`retries = N` means N total attempts) |
| **Scheduling**     | Runs right now                              | Delayed `run_at` and cron                       |
| **Primary Target** | Internal app lifecycle, WebSockets, Metrics | Billing, email sending, PDF exports, migrations |
