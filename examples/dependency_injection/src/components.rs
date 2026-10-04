//! Components: the things the container knows how to build.
//!
//! Three of the four DI entry points live here, so the guide can compare them
//! on the same types.

use gritshield::{component, mark_injectable, GritComponent};
use std::sync::Arc;

// An explicit registration: this type is *not* a component, it is a value you
// already have (a config struct, a third-party client, a parsed settings
// bundle). `mark_injectable!` authorizes it for dynamic injection - without
// this line, `inject!` still stores it at runtime but any handler asking for it
// fails to compile.
//
// It must be at module scope, not inside a function: it expands to an `impl`.
mark_injectable!(AppConfig);

#[derive(Clone)]
pub struct AppConfig {
    pub currency: String,
    pub max_amount_cents: i64,
}

// ---------------------------------------------------------------------------
// 1. `#[component]` - constructor injection
// ---------------------------------------------------------------------------

/// The leaf of the graph. No dependencies, so nothing can go wrong while
/// building it, also `#[component]` is defined on impl.
pub struct DatabasePool {
    pub label: String,
}

#[component]
impl DatabasePool {
    pub fn new() -> Self {
        Self {
            label: "in-memory".to_string(),
        }
    }

    pub fn query(&self, sql: &str) -> String {
        format!("[{}] {}", self.label, sql)
    }
}

/// A component that depends on two others: one by `Arc`, one by value.
///
/// `db: Arc<DatabasePool>` is taken as-is from the container. `config:
/// AppConfig` is *cloned out* of the container's `Arc<AppConfig>` - the macro
/// rewrites a non-`Arc` parameter into `(*resolved).clone()`, which is why
/// `AppConfig` derives `Clone`.
pub struct PaymentService {
    db: Arc<DatabasePool>,
    config: AppConfig,
}

#[component]
impl PaymentService {
    pub fn new(db: Arc<DatabasePool>, config: AppConfig) -> Self {
        Self { db, config }
    }

    pub fn charge(&self, amount_cents: i64) -> Result<String, String> {
        if amount_cents > self.config.max_amount_cents {
            return Err(format!(
                "{} exceeds the {} limit",
                amount_cents, self.config.max_amount_cents
            ));
        }
        Ok(format!(
            "charged {} {} via {}",
            amount_cents,
            self.config.currency,
            self.db.query("INSERT INTO charges")
        ))
    }
}

// ---------------------------------------------------------------------------
// 2. `#[derive(GritComponent)]` - field injection
// ---------------------------------------------------------------------------

/// Same dependencies as `PaymentService`, declared as struct fields instead of
/// constructor parameters. Use it when the object is mostly a bag of
/// collaborators and a constructor would just be a field list.
///
/// The controller in `dynamic.rs` does *not* construct this. `Router::new()`
/// builds it during the DI boot phase and hands it to every handler that asks.
#[derive(Clone, GritComponent)]
pub struct InvoiceService {
    pub db: Arc<DatabasePool>,
    pub config: AppConfig,
}

impl InvoiceService {
    pub fn render(&self, customer: &str) -> String {
        format!(
            "invoice for {} (max {} {})",
            customer, self.config.max_amount_cents, self.config.currency
        )
    }
}
