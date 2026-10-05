//! The HTTP surface: place an order, watch the background work happen.

use crate::activity;
use crate::events::OrderPlaced;
use crate::jobs::{AlwaysFailsJob, FlakyChargeJob, SendInvoiceJob};
use gritshield::http::response::HttpStatus;
use gritshield::deps::serde::{Deserialize, Serialize};
use gritshield::prelude::*;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[derive(Deserialize, Serialize)]
pub struct PlaceOrder {
    pub email: String,
    pub total_cents: i64,
    pub succeed_on_attempt: u32,
}

static ORDER_SEQ: AtomicU32 = AtomicU32::new(0);

pub struct ApiController;

#[controller("/api")]
impl ApiController {
}