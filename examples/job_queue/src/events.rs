//! Paradigm one of two: the event bus.
//!
//! An event is a plain serializable struct. `#[derive(GritEvent)]` gives it a
//! name and a `publish()`; `#[event]` on an impl block containing
//! `handle(&self, event: Arc<YourEvent>)` subscribes that handler to the bus.
//!
//! Nothing registers handlers. `Router::new()` calls `EventBus::auto_discover()`,
//! which walks the inventory that `#[event]` submitted at compile time - so a
//! handler in a file you have never opened is live by the time the server binds.
//! Two handlers can subscribe to the same event, and both run.

use crate::activity;
use gritshield::deps::serde::{Deserialize, Serialize};
use gritshield::event;
use gritshield::GritEvent;
use std::sync::Arc;

/// The event itself. `Clone` and the serde derives are not required by the
/// `GritEvent` trait, but `Clone` is what lets you call `event.publish()`.
#[derive(GritEvent, Clone, Serialize, Deserialize)]
pub struct OrderPlaced {
    pub order_id: String,
    pub email: String,
    pub total_cents: i64,
}

/// Handler #1. Every `OrderPlaced` on the bus reaches this, on a task the
/// `register_handler` call spawned - never on the request thread.
pub struct ReceiptEmailer;

#[event]
impl ReceiptEmailer {
    pub async fn handle(&self, event: Arc<OrderPlaced>) {
        activity::record(
            "event/receipt",
            format!(
                "emailing receipt for {} to {} ({} cents)",
                event.order_id, event.email, event.total_cents
            ),
        );
    }
}

/// Handler #2, subscribed to the same event type, proving the bus fans out.
pub struct LedgerAppender;

#[event]
impl LedgerAppender {
    pub async fn handle(&self, event: Arc<OrderPlaced>) {
        activity::record(
            "event/ledger",
            format!("appending {} to the ledger", event.order_id),
        );
    }
}
