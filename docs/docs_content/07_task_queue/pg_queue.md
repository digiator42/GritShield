
### Step 1: Database Migration / Schema

To store jobs reliably in PostgreSQL, we create a `grit_jobs` table:

SQL

```sql
CREATE TABLE grit_jobs (
    id VARCHAR(64) PRIMARY KEY,
    job_type VARCHAR(255) NOT NULL,
    payload BYTEA NOT NULL,
    max_retries INT NOT NULL DEFAULT 3,
    current_attempts INT NOT NULL DEFAULT 0,
    run_at BIGINT NOT NULL,
    status VARCHAR(32) NOT NULL DEFAULT 'pending', -- 'pending', 'processing'
    last_error TEXT,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

-- Index to optimize worker polling queries
CREATE INDEX idx_grit_jobs_polling ON grit_jobs (status, run_at);
```

### Step 2: Implementing `PostgresJobQueue` (`JobStorage`)

This implementation uses PostgreSQL's **`FOR UPDATE SKIP LOCKED`** pattern. This is the industry-standard way to allow multiple web servers/workers to safely pull jobs from the same database table concurrently without race conditions or processing the same job twice!

Rust

```rust
use async_trait::async_trait;
use sea_orm::{DatabaseConnection, ConnectionTrait, Statement, DbBackend};
use std::sync::Arc;
use std::fmt::Debug;
use gritshield::core::event_bus::{JobEnvelope, JobStorage};

pub struct PostgresJobQueue {
    db: Arc<DatabaseConnection>,
}

impl PostgresJobQueue {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

impl Debug for PostgresJobQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresJobQueue").finish()
    }
}

#[async_trait]
impl JobStorage for PostgresJobQueue {
    async fn enqueue(&self, job: JobEnvelope) -> Result<(), String> {
        let query = format!(
            "INSERT INTO grit_jobs (id, job_type, payload, max_retries, current_attempts, run_at, status)
             VALUES ('{}', '{}', $1, {}, {}, {}, 'pending')",
            job.id, job.job_type, job.max_retries, job.current_attempts, job.run_at
        );

        self.db
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                &query,
                vec![job.payload.into()],
            ))
            .await
            .map_err(|e| format!("DB Enqueue Error: {}", e))?;

        Ok(())
    }

    async fn fetch_next(&self) -> Result<Option<JobEnvelope>, String> {
        let now = chrono::Utc::now().timestamp();

        // Atomically selects AND locks the next ready job, skipping locked rows used by other workers
        let query = format!(
            "UPDATE grit_jobs
             SET status = 'processing'
             WHERE id = (
                 SELECT id FROM grit_jobs
                 WHERE status = 'pending' AND run_at <= {}
                 ORDER BY run_at ASC
                 FOR UPDATE SKIP LOCKED
                 LIMIT 1
             )
             RETURNING id, job_type, payload, max_retries, current_attempts, run_at;",
            now
        );

        let query_res = self.db
            .query_one(Statement::from_string(DbBackend::Postgres, query))
            .await
            .map_err(|e| format!("DB Fetch Error: {}", e))?;

        if let Some(row) = query_res {
            let id: String = row.try_get("", "id").map_err(|e| e.to_string())?;
            let job_type: String = row.try_get("", "job_type").map_err(|e| e.to_string())?;
            let payload: Vec<u8> = row.try_get("", "payload").map_err(|e| e.to_string())?;
            let max_retries: i32 = row.try_get("", "max_retries").map_err(|e| e.to_string())?;
            let current_attempts: i32 = row.try_get("", "current_attempts").map_err(|e| e.to_string())?;
            let run_at: i64 = row.try_get("", "run_at").map_err(|e| e.to_string())?;

            Ok(Some(JobEnvelope {
                id,
                job_type,
                payload,
                max_retries: max_retries as u32,
                current_attempts: current_attempts as u32,
                run_at,
            }))
        } else {
            Ok(None)
        }
    }

    async fn complete(&self, job_id: &str) -> Result<(), String> {
        // Delete finished job from the database table
        let query = format!("DELETE FROM grit_jobs WHERE id = '{}'", job_id);
        self.db
            .execute(Statement::from_string(DbBackend::Postgres, query))
            .await
            .map_err(|e| format!("DB Complete Error: {}", e))?;
        Ok(())
    }

    async fn fail(&self, job: JobEnvelope, error: &str) -> Result<(), String> {
        eprintln!("[DB JOB RETRY] Job ID: {} | Error: {}", job.id, error);

        // Put the job back in 'pending' status with updated run_at timestamp for retry backoff
        let query = format!(
            "UPDATE grit_jobs 
             SET status = 'pending', current_attempts = {}, run_at = {}, last_error = $1 
             WHERE id = '{}'",
            job.current_attempts, job.run_at, job.id
        );

        self.db
            .execute(Statement::from_sql_and_values(
                DbBackend::Postgres,
                &query,
                vec![error.into()],
            ))
            .await
            .map_err(|e| format!("DB Retry Error: {}", e))?;

        Ok(())
    }
}
```

### Step 3: Define Your Job & Controller

Nothing changes here! write standard business logic:

Rust

```rust
use std::time::Duration;
use gritshield::prelude::*;

// 1. Define Job
#[derive(Serialize, Deserialize, GritJob)]
pub struct ProcessInvoiceJob {
    pub invoice_id: String,
    pub amount: f64,
}

#[job(name = "process_invoice", retries = 3)]
impl ProcessInvoiceJob {
    pub async fn perform(&self) -> Result<(), String> {
        println!(
            " [WORKER] Processing Invoice #{} for ${:.2} via PostgreSQL!",
            self.invoice_id, self.amount
        );
        Ok(())
    }
}

// 2. Controller
pub struct InvoiceController;

#[controller("/api/invoices")]
impl InvoiceController {
    #[post("/process")]
    pub async fn process(ctx: RequestContext) -> Response {
        let job = ProcessInvoiceJob {
            invoice_id: "INV-99201".to_string(),
            amount: 250.75,
        };

        if let Some(queue) = &ctx.job_queue {
            let _ = job.enqueue_in(queue, Duration::from_secs(10)).await;
        }

        Response::ok("Invoice processing job persisted to PostgreSQL!")
    }
}
```

### Step 4: Bootstrapping in `main.rs`

```rust
let db = Arc::new(Database::connect("postgres://postgres:password@localhost:5432/db").await?);
let router = Router::new().with_job_queue(Arc::new(PostgresJobQueue::new(db)), 10);
```