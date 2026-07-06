use serde::Serialize;
use chrono::{DateTime, Utc};
use maud::Markup;
use maud::html;
use crate::prelude::*;
use crate::protocol::response::{IntoResponseBody, JsonPayload};

#[derive(Serialize, Debug)]
pub struct AppMetrics {
    pub status: &'static str,
    pub timestamp: DateTime<Utc>,
    pub system: SystemCpuRamMetrics,
    pub database: DatabasePoolMetrics,
    pub process: ProcessMetrics,
}

#[derive(Serialize, Debug)]
pub struct SystemCpuRamMetrics {
    pub total_memory_mb: u64,
    pub used_memory_mb: u64,
    pub memory_percentage: f32,
    pub cpu_global_usage: f32,
    pub core_count: usize,
}

#[derive(Serialize, Debug)]
pub struct DatabasePoolMetrics {
    pub status: &'static str,
    pub backend: String,
    pub response_time_ms: u128,
}

#[derive(Serialize, Debug)]
pub struct ProcessMetrics {
    pub uptime_seconds: u64,
    pub pid: u32,
}

use sea_orm::{DatabaseConnection, ConnectionTrait, Statement};
use sysinfo::{System, Process, Pid};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

// Keep track of application startup time globally
lazy_static::lazy_static! {
    pub static ref START_TIME: Instant = Instant::now();
}

pub async fn gather_all_metrics(db: &DatabaseConnection) -> AppMetrics {
    // Initialize local system snapshot
    let mut sys = System::new_all();
    sys.refresh_all();

    // Compute Memory metrics
    let total_mem = sys.total_memory() / 1024 / 1024; // Convert bytes to MB
    let used_mem = sys.used_memory() / 1024 / 1024;
    let mem_pct = if total_mem > 0 { (used_mem as f32 / total_mem as f32) * 100.0 } else { 0.0 };

    let system_stats = SystemCpuRamMetrics {
        total_memory_mb: total_mem,
        used_memory_mb: used_mem,
        memory_percentage: mem_pct,
        cpu_global_usage: sys.global_cpu_usage(),
        core_count: sys.cpus().len(),
    };

    // Evaluate Database Pool and Query Latency
    let db_start = Instant::now();
    let db_status = match db.execute(Statement::from_string(db.get_database_backend(), "SELECT 1;")).await {
        Ok(_) => "healthy",
        Err(_) => "unhealthy",
    };
    let db_latency = db_start.elapsed().as_millis();

    let db_stats = DatabasePoolMetrics {
        status: db_status,
        backend: format!("{:?}", db.get_database_backend()),
        response_time_ms: db_latency,
    };

    // Capture Self Process Stats
    let current_pid = std::process::id();
    let uptime = START_TIME.elapsed().as_secs();

    let process_stats = ProcessMetrics {
        uptime_seconds: uptime,
        pid: current_pid,
    };

    AppMetrics {
        status: if db_status == "healthy" { "operational" } else { "degraded" },
        timestamp: Utc::now(),
        system: system_stats,
        database: db_stats,
        process: process_stats,
    }
}


pub fn render_metrics_dashboard(metrics: &AppMetrics) -> Markup {
    html! {
        div class="space-y-6 p-6 animate-slide-in" id="metrics-panel" 
            hx-get="/admin/api/metrics/html" 
            hx-trigger="every 5s" 
            hx-swap="outerHTML" {
            
            // Upper Summary Status Bar
            div class="flex items-center justify-between border-b border-gray-800 pb-4" {
                div {
                    h2 class="text-sm font-bold font-mono text-gray-200" { "🎛️ Real-Time Core Telemetry" }
                    p class="text-xxs font-mono text-gray-500 mt-0.5" { (format!("Last monitored pull: {}", metrics.timestamp.format("%Y-%m-%d %H:%M:%S UTC"))) }
                }
                
                @if metrics.status == "operational" {
                    span class="px-2.5 py-1 bg-emerald-950/40 border border-emerald-800/80 text-emerald-400 font-mono text-xxs rounded-full animate-pulse" { "● SYSTEM OPERATIONAL" }
                } @else {
                    span class="px-2.5 py-1 bg-red-950/40 border border-red-800/80 text-red-400 font-mono text-xxs rounded-full" { "▲ SYSTEM DEGRADED" }
                }
            }

            // Grid Layout Metric Cards
            div class="grid grid-cols-1 md:grid-cols-3 gap-4" {
                
                // Card 1: Global CPU Usage Core Load
                div class="p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-3 shadow-sm" {
                    div class="flex justify-between items-center" {
                        span class="text-xxs font-mono uppercase text-gray-500 tracking-wider" { "CPU Capacity Load" }
                        span class="text-xs font-mono text-blue-400 font-bold" { (format!("{:.1}%", metrics.system.cpu_global_usage)) }
                    }
                    div class="w-full bg-gray-900 rounded-full h-1.5 overflow-hidden" {
                        div class="bg-blue-500 h-1.5 rounded-full transition-all duration-500" style=(format!("width: {}%", metrics.system.cpu_global_usage)) {}
                    }
                    p class="text-[10px] font-mono text-gray-400" { (format!("Logical Cores Detected: {} Processing Vectors", metrics.system.core_count)) }
                }

                // Card 2: Memory/RAM Consumption Tracking Frame
                div class="p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-3 shadow-sm" {
                    div class="flex justify-between items-center" {
                        span class="text-xxs font-mono uppercase text-gray-500 tracking-wider" { "RAM Memory Footprint" }
                        span class="text-xs font-mono text-purple-400 font-bold" { (format!("{:.1}%", metrics.system.memory_percentage)) }
                    }
                    div class="w-full bg-gray-900 rounded-full h-1.5 overflow-hidden" {
                        div class="bg-purple-500 h-1.5 rounded-full transition-all duration-500" style=(format!("width: {}%", metrics.system.memory_percentage)) {}
                    }
                    p class="text-[10px] font-mono text-gray-400" { (format!("Allocated: {} MB / Total: {} MB", metrics.system.used_memory_mb, metrics.system.total_memory_mb)) }
                }

                // Card 3: Database Pool Gateway Ping Latency
                div class="p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-3 shadow-sm" {
                    div class="flex justify-between items-center" {
                        span class="text-xxs font-mono uppercase text-gray-500 tracking-wider" { "Database Pool Latency" }
                        span class="text-xs font-mono text-emerald-400 font-bold" { (format!("{} ms", metrics.database.response_time_ms)) }
                    }
                    div class="w-full bg-gray-900 rounded-full h-1.5 overflow-hidden" {
                        @let lat_bar = std::cmp::min(metrics.database.response_time_ms, 100) as f32;
                        div class="bg-emerald-500 h-1.5 rounded-full transition-all duration-500" style=(format!("width: {}%", lat_bar)) {}
                    }
                    p class="text-[10px] font-mono text-gray-400" { (format!("Engine Driver Backend: {} Driver Connection", metrics.database.backend)) }
                }
            }

            // Lower Operational Uptime Metadata Block
            div class="p-3 bg-gray-900/40 border border-gray-800/60 rounded-xl flex items-center justify-between text-xxs font-mono text-gray-400" {
                span { (format!("Server OS Runtime Context PID [ {} ]", metrics.process.pid)) }
                span { (format!("Continuous Application Uptime: {} seconds", metrics.process.uptime_seconds)) }
            }
        }
    }
}

// Inside your main panel routing views
pub fn render_admin_settings_or_home_page(metrics: &AppMetrics) -> Markup {
    html! {
        div id="main-content" class="p-6" {
            h1 class="text-xl font-bold font-mono text-gray-100 mb-6" { "System Settings & Health" }
            
            // Seed the initial data on page load. 
            // Every 5 seconds, this exact wrapper will hit /admin/api/metrics/html 
            // and swap itself out with fresh layout frames automatically!
            (render_metrics_dashboard(metrics))
        }
    }
}

// GET /admin/api/metrics
pub async fn admin_metrics_api_handler(ctx: RequestContext) -> Response {

    // Fetch real-time snapshot metrics safely
    let metrics_payload = gather_all_metrics(&ctx.db.as_ref().unwrap()).await;

    // Serialize back out into a structured standard JSON text layout
    Response::json(200, &JsonPayload(serde_json::to_string_pretty(&metrics_payload).unwrap()))
}

// GET /admin/api/metrics/html
pub async fn admin_metrics_html_handler(ctx: RequestContext) -> Response {
    // 1. Fetch real-time snapshot metrics safely
    let metrics_payload = gather_all_metrics(&ctx.db.as_ref().unwrap()).await;

    // 2. Render the Maud template code component into a raw String
    let rendered_html = render_metrics_dashboard(&metrics_payload).into_string();

    // 3. Return the response marked explicitly as HTML payload
    let mut res = Response::ok(rendered_html);
    res.headers.push(("content-type".to_string(), "text/html; charset=utf-8".to_string()));
    res
}