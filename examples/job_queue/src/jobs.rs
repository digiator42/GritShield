//! Paradigm two of two: the job queue.
//!
//! A job is a serializable struct with an inherent `async fn perform`. The
//! `#[job]` attribute implements `GritJob` for it, adds inherent `enqueue()` /
//! `enqueue_in()` methods, and submits a registration to the same inventory the
//! event bus uses - which is how the worker finds your `perform` again at
//! dispatch time.
//!
//! `ignite` starts the machinery: a `JobWorkerEngine` with ten workers and a
//! `CronScheduler`, both polling `router.job_queue`. There is nothing to wire,
//! and nothing to shut down.

use crate::activity;
use crate::events::OrderPlaced;
use gritshield::deps::serde::{Deserialize, Serialize};
use gritshield::job;
use gritshield::GritJob;
use std::sync::atomic::{AtomicU32, Ordering};

// ─────────────────────────────────────────────────────────────────────────────
// A job that works
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, GritJob)]
pub struct SendInvoiceJob {
    pub order_id: String,
    pub email: String,
}

/// `retries = 3` is the only knob `#[job]` exposes besides `name` and `cron`.
/// Without it the trait default of 3 applies, so stating it here is for the
/// reader, not the compiler.
#[job(retries = 3)]
impl SendInvoiceJob {
    pub async fn perform(&self) -> Result<(), String> {
        activity::record(
            "job/invoice",
            format!("sending invoice for {} to {}", self.order_id, self.email),
        );

        // Jobs and events are the same bus. The worker scopes the event bus as
        // a task-local before calling `perform`, which is why `publish()` works
        // in here with no bus argument at all.
        OrderPlaced {
            order_id: self.order_id.clone(),
            email: self.email.clone(),
            total_cents: 0,
        }
        .publish()
        .await;

        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A job that fails, then succeeds: exponential backoff
// ─────────────────────────────────────────────────────────────────────────────

/// Survives across attempts and across requests, because it is a plain static.
/// A job is deserialized fresh from JSON on every attempt, so any state it needs
/// has to live outside the struct.
static FLAKY_ATTEMPTS: AtomicU32 = AtomicU32::new(0);

#[derive(Serialize, Deserialize, GritJob)]
pub struct FlakyChargeJob {
    pub order_id: String,
    pub succeed_on_attempt: u32,
}

#[job(retries = 4)]
impl FlakyChargeJob {
    pub async fn perform(&self) -> Result<(), String> {
        let attempt = FLAKY_ATTEMPTS.fetch_add(1, Ordering::SeqCst) + 1;

        if attempt < self.succeed_on_attempt {
            activity::record(
                "job/flaky",
                format!(
                    "attempt {}/{} for {} failed, will retry",
                    attempt, self.succeed_on_attempt, self.order_id
                ),
            );
            return Err(format!("upstream gateway refused attempt {attempt}"));
        }

        activity::record(
            "job/flaky",
            format!("attempt {} for {} finally went through", attempt, self.order_id),
        );
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A job that never succeeds: the dead-letter path
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, GritJob)]
pub struct AlwaysFailsJob {
    pub order_id: String,
}

/// `retries = 2` means two attempts total, then the engine gives up.
#[job(retries = 2)]
impl AlwaysFailsJob {
    pub async fn perform(&self) -> Result<(), String> {
        activity::record(
            "job/doomed",
            format!("attempt for {} failed, unrecoverable", self.order_id),
        );
        Err("ledger is offline".to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// A cron job
// ─────────────────────────────────────────────────────────────────────────────

/// Note what this is *not*: a struct with fields. `CronScheduler` enqueues cron
/// jobs with the payload `null`, because it has no parameters to pass - the
/// enqueue happens on a timer, not from a request. A cron job whose type needs
/// fields deserializes `null` at dispatch time and fails forever, one wasted
/// worker slot per attempt.
#[derive(Serialize, Deserialize, GritJob)]
pub struct Heartbeat;

/// Six fields: second, minute, hour, day, month, weekday. Validated at compile
/// time - `#[job(cron = "not a cron")]` fails the build.
#[job(cron = "* * * * * *")]
impl Heartbeat {
    pub async fn perform(&self) -> Result<(), String> {
        activity::record("cron/heartbeat", "tick");
        Ok(())
    }
}
