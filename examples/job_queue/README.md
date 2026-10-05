# Job Queue

Background work: events, jobs, retries, and a cron tick in one runnable example.

`ash
cargo run --manifest-path examples/job_queue/Cargo.toml
`

GET /api/activity lets you observe the interleaving of HTTP responses, event handlers, jobs and cron ticks without tailing logs. Use DELETE /api/activity to reset between runs.

## What it shows

| Layer | Route/Item | What happens |
|---|---|---|
| Events | #[derive(GritEvent)] + #[event] | Two handlers (ReceiptEmailer, LedgerAppender) subscribe to OrderPlaced and run on tasks spawned by the event bus. |
| Jobs (immediate) | SendInvoiceJob::enqueue() | Dispatched by JobWorkerEngine (10 workers), runs in background, and its perform() can publish events using task-local context. |
| Jobs (delayed) | SendInvoiceJob::enqueue_in(Duration::from_secs(3)) | Envelope stores un_at; the in-memory storage re-enqueues delayed jobs until their time arrives (polled every ~250ms). |
| Retries/backoff | FlakyChargeJob | On failure, the engine computes exponential backoff (2^attempt) and re-enqueues; the example succeeds on attempt 3 to show the sequence. |
| Dead letter | AlwaysFailsJob (retries=2) | After exhausting retries, the engine logs the failure and treats the job as terminal (removed) in the memory store. |
| Cron | Heartbeat with cron = "* * * * * *" | CronScheduler ticks once per second and enqueues cron jobs; the engine executes them. |

## Try it

`ash
# 1) Clear and place an order
curl -X DELETE http://127.0.0.1:8086/api/activity >/dev/null 2>&1; \
curl -X POST http://127.0.0.1:8086/api/orders \
  -H 'Content-Type: application/json' \
  -d '{\"email\":\"ada@example.com\",\"total_cents\":4200,\"succeed_on_attempt\":3}'

# 2) Wait long enough for delayed+retries+cron to tick, then inspect
sleep 8; curl -s http://127.0.0.1:8086/api/activity
`

The response to POST /api/orders is 202 Accepted immediately. The background interleaving - event handlers firing, job attempts failing/retrying, the cron ticks, and the delayed job landing - appears in /api/activity in timestamp order.

## How it's wired

- Router::new() calls EventBus::auto_discover() so every #[event] impl in the binary is registered.
- ignite spawns JobWorkerEngine (reading from outer.job_queue, using MemoryJobQueue by default) and CronScheduler (reading cron job registrations).
- Event publishing inside a job works because the worker scopes CURRENT_EVENT_BUS and CURRENT_JOB_QUEUE as task-locals before perform() runs.
- nqueue_in uses transaction/task-local context when present; otherwise it enqueues directly into the queue from context. The worker's retry sets un_at to 
ow + 2^attempt.