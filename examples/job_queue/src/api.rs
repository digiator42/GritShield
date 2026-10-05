use crate::activity;
use crate::events::OrderPlaced;
use crate::jobs::{AlwaysFailsJob, FlakyChargeJob, SendInvoiceJob};
use gritshield::http::response::HttpStatus;
use gritshield::http::response::Response;
use gritshield::prelude::*;
use gritshield::deps::serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;