use sea_orm_migration::async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::Debug;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;
use tokio::time::sleep;
pub struct EventRegistration {
    pub event_type: &'static str,   // e.g., "UserRegistered"
    pub handler_type: &'static str, // e.g., "WelcomeEmailHandler"
    pub register: fn(&EventBus),
}

inventory::collect!(EventRegistration);

#[derive(Debug)]
pub struct EventBus {
    // Allows both internal async channels and multi-subscriber dispatches
    sender: broadcast::Sender<Arc<dyn Any + Send + Sync>>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    pub fn init() -> Self {
        Self::new(1024)
    }

    /// Iterates over all compile-time inventory submissions and binds them
    pub fn auto_discover(&self) {
        for registration in inventory::iter::<EventRegistration> {
            (registration.register)(self);
        }
    }

    pub fn publish<E: GritEvent>(&self, event: E) {
        let payload = Arc::new(event);
        let _ = self.sender.send(payload);
    }

    /// Automatically manages channel subscriptions and downcasting in the background.
    pub fn register_handler<E, H>(&self, handler: H)
    where
        E: GritEvent,
        H: GritEventHandler<E> + 'static,
    {
        let mut rx = self.sender.subscribe();
        let erased_wrapper = Arc::new(ErasedHandlerWrapper::new(Arc::new(handler)));

        tokio::spawn(async move {
            while let Ok(payload) = rx.recv().await {
                // Safely downcasts and executes handler if payload matches type E
                erased_wrapper.dispatch(payload).await;
            }
        });
    }

    /// Pass an async closure directly.
    pub fn register_fn<E, F, Fut>(&self, f: F)
    where
        E: GritEvent,
        F: Fn(Arc<E>) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        struct FnHandler<F>(F);

        #[async_trait]
        impl<E, F, Fut> GritEventHandler<E> for FnHandler<F>
        where
            E: GritEvent,
            F: Fn(Arc<E>) -> Fut + Send + Sync + 'static,
            Fut: std::future::Future<Output = ()> + Send + 'static,
        {
            async fn handle(&self, event: Arc<E>) {
                (self.0)(event).await;
            }
        }

        self.register_handler::<E, _>(FnHandler(f));
    }
}

pub trait GritEvent: Send + Sync + 'static {
    fn event_name() -> &'static str
    where
        Self: Sized;
}

#[async_trait]
pub trait GritEventHandler<E: GritEvent>: Send + Sync + 'static {
    async fn handle(&self, event: Arc<E>);
}

pub type JobFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

pub struct JobRegistration {
    pub job_type: &'static str,
    pub handler_type: &'static str,
    pub max_retries: u32,
    pub execute: fn(payload: &[u8]) -> JobFuture,
}

inventory::collect!(JobRegistration);

// Erased handler wrapper for the DI event registry
#[async_trait]
pub trait ErasedEventHandler: Send + Sync + 'static {
    async fn dispatch(&self, event: Arc<dyn Any + Send + Sync>);
}

// Native Async Job Queue & Worker Engine
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct JobEnvelope {
    pub id: String,
    pub job_type: String,
    pub payload: Vec<u8>,
    pub max_retries: u32,
    pub current_attempts: u32,
    pub run_at: i64, // Unix timestamp for scheduled/delayed jobs
}

#[async_trait]
pub trait GritJob: Serialize + for<'de> Deserialize<'de> + Send + Sync {
    const NAME: &'static str;

    async fn perform(&self) -> Result<(), String>;

    fn max_retries(&self) -> u32 {
        3
    }
    fn backoff_delay(&self, attempt: u32) -> Duration {
        Duration::from_secs(2u64.pow(attempt)) // Exponential backoff
    }
}

// Abstraction Layer for Queue Backends (JobStorage)
#[async_trait]
pub trait JobStorage: Send + Sync + Debug + 'static {
    async fn enqueue(&self, job: JobEnvelope) -> Result<(), String>;
    async fn fetch_next(&self) -> Result<Option<JobEnvelope>, String>;
    async fn complete(&self, job_id: &str) -> Result<(), String>;
    async fn fail(&self, job: JobEnvelope, error: &str) -> Result<(), String>;
}

#[derive(Debug)]
pub struct MemoryJobQueue {
    tx: mpsc::UnboundedSender<JobEnvelope>,
    rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<JobEnvelope>>,
}

impl MemoryJobQueue {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            tx,
            rx: tokio::sync::Mutex::new(rx),
        }
    }
}

pub struct JobWorkerEngine {
    storage: Arc<dyn JobStorage>,
    concurrency_limit: Arc<Semaphore>,
}

impl JobWorkerEngine {
    pub fn new(storage: Arc<dyn JobStorage>, max_workers: usize) -> Self {
        Self {
            storage,
            concurrency_limit: Arc::new(Semaphore::new(max_workers)),
        }
    }

    pub async fn start(&self) {
        loop {
            // Respect concurrency pool limits
            let permit = self
                .concurrency_limit
                .clone()
                .acquire_owned()
                .await
                .unwrap();
            let storage = self.storage.clone();

            if let Ok(Some(mut job)) = storage.fetch_next().await {
                tokio::spawn(async move {
                    job.current_attempts += 1;

                    // Execute Job
                    match Self::dispatch_job(&job).await {
                        Ok(_) => {
                            let _ = storage.complete(&job.id).await;
                        }
                        Err(err) => {
                            if job.current_attempts < job.max_retries {
                                // Exponential backoff retry
                                let delay = 2u64.pow(job.current_attempts);
                                job.run_at = chrono::Utc::now().timestamp() + delay as i64;
                                let _ = storage.fail(job, &err).await;
                            } else {
                                eprintln!(
                                    "[JOB DEAD-LETTER] Job {} exceeded max retries: {}",
                                    job.id, err
                                );
                                let _ = storage.complete(&job.id).await;
                            }
                        }
                    }
                    drop(permit); // Release slot back to pool
                });
            } else {
                drop(permit);
                sleep(std::time::Duration::from_millis(250)).await; // Idle polling sleep
            }
        }
    }

    async fn dispatch_job(job: &JobEnvelope) -> Result<(), String> {
        for reg in inventory::iter::<JobRegistration> {
            if reg.job_type == job.job_type {
                return (reg.execute)(&job.payload).await;
            }
        }

        Err(format!(
            "No registered job runner found for job type: '{}'",
            job.job_type
        ))
    }
}

// Adapter struct that wraps any typed GritEventHandler into an ErasedEventHandler
pub struct ErasedHandlerWrapper<E: GritEvent, H: GritEventHandler<E>> {
    pub handler: Arc<H>,
    _phantom: std::marker::PhantomData<E>,
}

impl<E: GritEvent, H: GritEventHandler<E>> ErasedHandlerWrapper<E, H> {
    pub fn new(handler: Arc<H>) -> Self {
        Self {
            handler,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<E, H> ErasedEventHandler for ErasedHandlerWrapper<E, H>
where
    E: GritEvent,
    H: GritEventHandler<E>,
{
    async fn dispatch(&self, event: Arc<dyn Any + Send + Sync>) {
        if let Ok(concrete_event) = event.downcast::<E>() {
            self.handler.handle(concrete_event).await;
        }
    }
}

/// Helper extension trait auto-implemented for all GritJobs
#[async_trait]
pub trait GritJobExt: GritJob {
    /// Enqueue job for immediate execution
    async fn enqueue(&self, queue: &Arc<dyn JobStorage>) -> Result<String, String> {
        let payload = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        let job_id = uuid::Uuid::new_v4().to_string();

        let envelope = JobEnvelope {
            id: job_id.clone(),
            job_type: Self::NAME.to_string(),
            payload,
            max_retries: self.max_retries(),
            current_attempts: 0,
            run_at: chrono::Utc::now().timestamp(),
        };

        queue.enqueue(envelope).await?;
        Ok(job_id)
    }

    /// Enqueue job with a scheduled delay
    async fn enqueue_in(
        &self,
        queue: &Arc<dyn JobStorage>,
        delay: Duration,
    ) -> Result<String, String> {
        let payload = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        let job_id = uuid::Uuid::new_v4().to_string();

        let envelope = JobEnvelope {
            id: job_id.clone(),
            job_type: Self::NAME.to_string(),
            payload,
            max_retries: self.max_retries(),
            current_attempts: 0,
            run_at: chrono::Utc::now().timestamp() + delay.as_secs() as i64,
        };

        queue.enqueue(envelope).await?;
        Ok(job_id)
    }
}

impl<T: GritJob> GritJobExt for T {}

#[async_trait]
impl JobStorage for MemoryJobQueue {
    async fn enqueue(&self, job: JobEnvelope) -> Result<(), String> {
        self.tx
            .send(job)
            .map_err(|e| format!("Failed to enqueue job: {}", e))
    }

    async fn fetch_next(&self) -> Result<Option<JobEnvelope>, String> {
        let mut rx = self.rx.lock().await;
        // try_recv prevents blocking the worker polling loop if queue is empty
        match rx.try_recv() {
            Ok(job) => {
                // Check if the scheduled run time has arrived
                let now = chrono::Utc::now().timestamp();
                if job.run_at <= now {
                    Ok(Some(job))
                } else {
                    // Job is delayed; put it back into the channel
                    let _ = self.tx.send(job);
                    Ok(None)
                }
            }
            Err(mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(mpsc::error::TryRecvError::Disconnected) => {
                Err("Job queue channel disconnected".to_string())
            }
        }
    }

    async fn complete(&self, _job_id: &str) -> Result<(), String> {
        // In-memory queue automatically pops items on fetch, so no-op here
        Ok(())
    }

    async fn fail(&self, job: JobEnvelope, error: &str) -> Result<(), String> {
        eprintln!("[JOB FAILED] ID: {} | Error: {}", job.id, error);
        // Re-enqueue the job for backoff retry execution
        self.enqueue(job).await
    }
}

pub struct EventBusGraph;
impl EventBusGraph {
    pub fn export_dot() -> String {
        let mut dot = String::new();

        dot.push_str("digraph GritshieldEventsJobs {\n");
        // Reduced rank separation and added global graph padding
        dot.push_str("  graph [rankdir=LR, bgcolor=\"#030712\", fontname=\"monospace\", fontsize=10, pad=\"0.3\", nodesep=\"0.4\", ranksep=\"0.5\"];\n");
        // CRITICAL FIX: Set explicit fontsize=10 and margin on nodes so text doesn't overflow shapes
        dot.push_str("  node [fontname=\"monospace\", fontsize=9, shape=box, style=\"filled,rounded\", penwidth=1.2, margin=\"0.15,0.1\"];\n");
        // Edge styling and label size
        dot.push_str("  edge [fontname=\"monospace\", fontsize=8, arrowsize=0.7];\n\n");

        // --- Subgraph 1: Background Jobs ---
        dot.push_str("  subgraph cluster_jobs {\n");
        dot.push_str("    label = \"Background Job Queue\";\n");
        dot.push_str("    fontsize = 10;\n");
        dot.push_str("    fontcolor = \"#c084fc\";\n");
        dot.push_str("    color = \"#581c87\";\n");
        dot.push_str("    style = \"dashed,rounded\";\n");
        dot.push_str("    margin = 12;\n");
        dot.push_str("    node [fillcolor=\"#1e1b4b\", color=\"#818cf8\", fontcolor=\"#e0e7ff\"];\n\n");

        for job in inventory::iter::<JobRegistration> {
            let clean_name = job.job_type.replace("::", "_");
            let job_node = format!("job_{}", clean_name);
            let worker_node = format!("worker_{}", clean_name);

            dot.push_str(&format!(
                "    {} [label=\"J: {}\\n(Retry Strategy)\", fillcolor=\"#31104b\", color=\"#a855f7\", fontcolor=\"#f3e8ff\"];\n",
                job_node, job.job_type
            ));

            dot.push_str(&format!(
                "    {} [label=\"{}\", fillcolor=\"#064e3b\", color=\"#34d399\", fontcolor=\"#ecfdf5\"];\n",
                worker_node, job.job_type
            ));

            dot.push_str(&format!(
                "    {} -> {} [label=\"executes\", color=\"#a855f7\", fontcolor=\"#c084fc\"];\n",
                job_node, worker_node
            ));
        }
        dot.push_str("  }\n\n");

        // --- Subgraph 2: Pub/Sub Event Bus ---
        dot.push_str("  subgraph cluster_events {\n");
        dot.push_str("    label = \"Event Bus System\";\n");
        dot.push_str("    fontsize = 11;\n");
        dot.push_str("    fontcolor = \"#38bdf8\";\n");
        dot.push_str("    color = \"#0369a1\";\n");
        dot.push_str("    style = \"dashed,rounded\";\n");
        dot.push_str("    margin = 12;\n");
        dot.push_str("    node [fillcolor=\"#0c4a6e\", color=\"#38bdf8\", fontcolor=\"#f0f9ff\"];\n\n");

        let mut has_events = false;
        for reg in inventory::iter::<EventRegistration> {
            has_events = true;
            let event_node = format!("evt_{}", reg.event_type.replace("::", "_"));
            let handler_node = format!("hnd_{}", reg.handler_type.replace("::", "_"));

            // Event Node
            dot.push_str(&format!(
                "    {} [label=\"E: {}\", fillcolor=\"#075985\", color=\"#0284c7\", fontcolor=\"#e0f2fe\"];\n",
                event_node, reg.event_type
            ));

            // Handler Node
            dot.push_str(&format!(
                "    {} [label=\"H: {}\", fillcolor=\"#14532d\", color=\"#22c55e\", fontcolor=\"#f0fdf4\"];\n",
                handler_node, reg.handler_type
            ));

            // Edge
            dot.push_str(&format!(
                "    {} -> {} [label=\"dispatches\", color=\"#38bdf8\", fontcolor=\"#7dd3fc\"];\n",
                event_node, handler_node
            ));
        }

        if !has_events {
            dot.push_str("    no_events [label=\"No Events Registered\", fillcolor=\"#1f2937\", color=\"#4b5563\", fontcolor=\"#9ca3af\"];\n");
        }

        dot.push_str("  }\n");
        dot.push_str("}\n");
        dot
    }
}