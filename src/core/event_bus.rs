use sea_orm_migration::async_trait::async_trait;
use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;
use tokio::sync::broadcast;
use crate::core::job_queue::JobRegistration;
use crate::database::repository::transaction::publish_event;


pub struct EventRegistration {
    pub event_type: &'static str,   // e.g., "UserRegistered"
    pub handler_type: &'static str, // e.g., "WelcomeEmailHandler"
    pub register: fn(&EventBus),
}

inventory::collect!(EventRegistration);

#[derive(Debug, Clone)]
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

    /// Publishes event directly to the bus, bypassing transaction staging.
    pub fn publish<E: GritEvent>(&self, event: E) {
        let payload = Arc::new(event);
        let _ = self.sender.send(payload);
    }

    /// Dispatches a erased event payload staged during transactions
    pub fn publish_erased_box(&self, payload: Box<dyn Any + Send + Sync>) {
        let arc_payload: Arc<dyn Any + Send + Sync> = payload.into();
        let _ = self.sender.send(arc_payload);
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

#[async_trait]
pub trait GritEventExt: GritEvent + Clone {
    /// Automatically stages inside `#[transactional]` or publishes directly to `CURRENT_EVENT_BUS`.
    async fn publish(self) {
        publish_event(self).await;
    }
}

// Blanket implementation for all GritEvents that derive Clone
impl<T: GritEvent + Clone> GritEventExt for T {}

// Erased handler wrapper for the DI event registry
#[async_trait]
pub trait ErasedEventHandler: Send + Sync + 'static {
    async fn dispatch(&self, event: Arc<dyn Any + Send + Sync>);
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
        dot.push_str(
            "    node [fillcolor=\"#1e1b4b\", color=\"#818cf8\", fontcolor=\"#e0e7ff\"];\n\n",
        );

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
        dot.push_str(
            "    node [fillcolor=\"#0c4a6e\", color=\"#38bdf8\", fontcolor=\"#f0f9ff\"];\n\n",
        );

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
