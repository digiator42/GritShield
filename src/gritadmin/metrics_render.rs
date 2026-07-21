use crate::prelude::*;
use crate::routing::engine::RequestContext;
use chrono::{DateTime, Utc};
use maud::html;
use maud::Markup;
use serde::Serialize;
use sea_orm::{ConnectionTrait, Statement};
use std::sync::atomic::Ordering;
use std::time::Instant;
use sysinfo::System;

#[derive(Serialize, Debug)]
pub struct HardeningMatrix {
    pub timestamp: DateTime<Utc>,
    pub ssl_active: bool,
    pub active_admin_sessions: usize,
    pub database_encryption_status: &'static str,
    pub max_login_attempts: u32,
    pub rate_limiting_degraded: bool,
    pub environment_mode: String,
    pub csp_enabled: bool,
    pub nosniff_enabled: bool,
    pub clickjacking_protected: bool,
    pub hsts_enabled: bool,
    pub current_request_authenticated: bool,
    pub incoming_cookies: Vec<InboundCookieDetails>,
}

#[derive(Serialize, Debug, Clone)]
pub struct InboundCookieDetails {
    pub name: String,
    pub value_preview: String,
    pub server_policy_compliance: &'static str,
}

#[derive(Serialize, Debug)]
pub struct EndpointSummary {
    pub path: String,
    pub requests: u64,
    pub avg_latency_ms: f64,
    pub errors_4xx: u64,
    pub errors_5xx: u64,
}

#[derive(Serialize, Debug)]
pub struct AppMetrics {
    pub status: &'static str,
    pub timestamp: DateTime<Utc>,
    pub system: SystemCpuRamMetrics,
    pub database: DatabasePoolMetrics,
    pub process: ProcessMetrics,
    // --- GritShield Real-Time Telemetry ---
    pub active_connections: u64,
    pub total_blocked_ips: u64,
    pub total_rate_limited_reqs: u64,
    pub total_allowed_reqs: u64,
    pub total_errors_4xx: u64,
    pub total_errors_5xx: u64,
    pub avg_latency_ms: f64,
    pub endpoints: Vec<EndpointSummary>,
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

lazy_static::lazy_static! {
    pub static ref START_TIME: Instant = Instant::now();
}

pub async fn gather_all_metrics(ctx: &RequestContext) -> AppMetrics {
    // Initialize local system snapshot
    let mut sys = System::new_all();
    sys.refresh_all();

    // Compute Memory metrics
    let total_mem = sys.total_memory() / 1024 / 1024;
    let used_mem = sys.used_memory() / 1024 / 1024;
    let mem_pct = if total_mem > 0 {
        (used_mem as f32 / total_mem as f32) * 100.0
    } else {
        0.0
    };

    let system_stats = SystemCpuRamMetrics {
        total_memory_mb: total_mem,
        used_memory_mb: used_mem,
        memory_percentage: mem_pct,
        cpu_global_usage: sys.global_cpu_usage(),
        core_count: sys.cpus().len(),
    };

    let db = ctx.db.as_ref().unwrap();

    // Evaluate Database Pool and Query Latency
    let db_start = Instant::now();
    let db_status = match db
        .execute(Statement::from_string(
            db.get_database_backend(),
            "SELECT 1;",
        ))
        .await
    {
        Ok(_) => "healthy",
        Err(_) => "unhealthy",
    };
    let db_latency = db_start.elapsed().as_millis();

    let db_stats = DatabasePoolMetrics {
        status: db_status,
        backend: format!("{:?}", db.get_database_backend()),
        response_time_ms: db_latency,
    };

    // Capture Process Stats
    let current_pid = std::process::id();
    let uptime = START_TIME.elapsed().as_secs();

    let process_stats = ProcessMetrics {
        uptime_seconds: uptime,
        pid: current_pid,
    };

    // Extract Atomic Telemetry
    let active_connections = ctx.telemetry.active_connections.load(Ordering::Relaxed);
    let total_blocked_ips = ctx.telemetry.total_blocked_ips.load(Ordering::Relaxed);
    let total_rate_limited_reqs = ctx.telemetry.total_rate_limited_reqs.load(Ordering::Relaxed);
    let total_allowed_reqs = ctx.telemetry.total_allowed_reqs.load(Ordering::Relaxed);
    let total_errors_4xx = ctx.telemetry.total_errors_4xx.load(Ordering::Relaxed);
    let total_errors_5xx = ctx.telemetry.total_errors_5xx.load(Ordering::Relaxed);
    let total_duration_micros = ctx.telemetry.total_duration_micros.load(Ordering::Relaxed);

    let avg_latency_ms = if total_allowed_reqs > 0 {
        (total_duration_micros as f64 / total_allowed_reqs as f64) / 1000.0
    } else {
        0.0
    };

    // Extract Endpoint Breakdown
    let mut endpoints = Vec::new();
    if let Ok(ep_map) = ctx.telemetry.endpoints.read() {
        for (path, metric) in ep_map.iter() {
            let reqs = metric.total_requests.load(Ordering::Relaxed);
            let dur = metric.total_duration_micros.load(Ordering::Relaxed);
            let e4 = metric.errors_4xx.load(Ordering::Relaxed);
            let e5 = metric.errors_5xx.load(Ordering::Relaxed);
            let ep_avg = if reqs > 0 { (dur as f64 / reqs as f64) / 1000.0 } else { 0.0 };

            endpoints.push(EndpointSummary {
                path: path.clone(),
                requests: reqs,
                avg_latency_ms: ep_avg,
                errors_4xx: e4,
                errors_5xx: e5,
            });
        }
    }
    endpoints.sort_by(|a, b| b.requests.cmp(&a.requests));

    AppMetrics {
        status: if db_status == "healthy" { "operational" } else { "degraded" },
        timestamp: Utc::now(),
        system: system_stats,
        database: db_stats,
        process: process_stats,
        active_connections,
        total_blocked_ips,
        total_rate_limited_reqs,
        total_allowed_reqs,
        total_errors_4xx,
        total_errors_5xx,
        avg_latency_ms,
        endpoints,
    }
}

pub fn render_metrics_dashboard(metrics: &AppMetrics) -> Markup {
    html! {
        div class="space-y-6 p-6 animate-slide-in" id="metrics-panel"
            hx-get="/admin/metrics"
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

            // --- GRITSHIELD CORE TELEMETRY TRACKER ---
            div class="bg-gray-950 border border-gray-800 rounded-xl p-4 space-y-3 shadow-md" {
                div class="flex justify-between items-center" {
                    span class="text-xxs font-mono uppercase text-gray-400 tracking-wider font-semibold" { "GritShield Network Guard Metrics" }
                    span class="w-2 h-2 rounded-full bg-emerald-500 animate-ping" {}
                }

                div class="grid grid-cols-2 md:grid-cols-4 gap-4 font-mono text-xxs" {
                    div class="p-3 bg-gray-900/40 border border-gray-800 rounded-lg" {
                        span class="text-gray-500 block mb-1" { "Active Connections" }
                        span class="text-xs font-bold text-blue-400" { (metrics.active_connections) }
                    }
                    div class="p-3 bg-gray-900/40 border border-gray-800 rounded-lg" {
                        span class="text-gray-500 block mb-1" { "Blocked Hack IPs" }
                        span class="text-xs font-bold text-red-400" { (metrics.total_blocked_ips) }
                    }
                    div class="p-3 bg-gray-900/40 border border-gray-800 rounded-lg" {
                        span class="text-gray-500 block mb-1" { "Rate-Limited Reqs" }
                        span class="text-xs font-bold text-amber-400" { (metrics.total_rate_limited_reqs) }
                    }
                    div class="p-3 bg-gray-900/40 border border-gray-800 rounded-lg" {
                        span class="text-gray-500 block mb-1" { "Allowed Clean Reqs" }
                        span class="text-xs font-bold text-emerald-400" { (metrics.total_allowed_reqs) }
                    }
                }
            }

            // --- NEW: HTTP PIPELINE PERFORMANCE MATRIX ---
            div class="bg-gray-950 border border-gray-800 rounded-xl p-4 space-y-3 shadow-md" {
                div class="flex justify-between items-center" {
                    span class="text-xxs font-mono uppercase text-gray-400 tracking-wider font-semibold" { "HTTP Execution & Latency Telemetry" }
                    span class="text-xxs font-mono text-cyan-400 font-bold" { (format!("Avg Latency: {:.2} ms", metrics.avg_latency_ms)) }
                }

                div class="grid grid-cols-1 md:grid-cols-3 gap-4 font-mono text-xxs" {
                    div class="p-3 bg-gray-900/40 border border-gray-800 rounded-lg flex items-center justify-between" {
                        span class="text-gray-400" { "Global Avg Latency" }
                        span class="text-xs font-bold text-cyan-400" { (format!("{:.2} ms", metrics.avg_latency_ms)) }
                    }
                    div class="p-3 bg-gray-900/40 border border-gray-800 rounded-lg flex items-center justify-between" {
                        span class="text-gray-400" { "4xx Client Errors" }
                        span class="text-xs font-bold text-amber-400" { (metrics.total_errors_4xx) }
                    }
                    div class="p-3 bg-gray-900/40 border border-gray-800 rounded-lg flex items-center justify-between" {
                        span class="text-gray-400" { "5xx Server Errors" }
                        span class="text-xs font-bold text-rose-400" { (metrics.total_errors_5xx) }
                    }
                }

                // Route breakdown table
                @if !metrics.endpoints.is_empty() {
                    div class="pt-2" {
                        div class="overflow-hidden border border-gray-850 rounded-lg" {
                            table class="w-full text-left font-mono text-xxs m-0 border-collapse" {
                                thead class="bg-gray-900 text-gray-400 uppercase tracking-wider" {
                                    tr {
                                        th class="p-2.5 font-semibold" { "Route Path" }
                                        th class="p-2.5 font-semibold" { "Requests" }
                                        th class="p-2.5 font-semibold" { "Avg Latency" }
                                        th class="p-2.5 font-semibold text-right" { "Errors (4xx / 5xx)" }
                                    }
                                }
                                tbody class="divide-y divide-gray-850" {
                                    @for ep in &metrics.endpoints {
                                        tr class="hover:bg-gray-900/40 transition" {
                                            td class="p-2.5 font-bold text-gray-300" { (ep.path) }
                                            td class="p-2.5 text-gray-400 font-mono" { (ep.requests) }
                                            td class="p-2.5 text-emerald-400 font-mono" { (format!("{:.2} ms", ep.avg_latency_ms)) }
                                            td class="p-2.5 text-right font-mono" {
                                                span class="text-amber-400" { (ep.errors_4xx) }
                                                span class="text-gray-600 px-1" { "/" }
                                                span class="text-rose-400" { (ep.errors_5xx) }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Grid Layout Metric Cards
            div class="grid grid-cols-1 md:grid-cols-3 gap-4" {

                // Card 1: CPU
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

                // Card 2: Memory
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

                // Card 3: Database
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

            // Lower Operational Uptime
            div class="p-3 bg-gray-900/40 border border-gray-800/60 rounded-xl flex items-center justify-between text-xxs font-mono text-gray-400" {
                span { (format!("Server OS Runtime Context PID [ {} ]", metrics.process.pid)) }
                span { (format!("Continuous Application Uptime: {} seconds", metrics.process.uptime_seconds)) }
            }
        }
    }
}

pub fn render_hardening_matrix(matrix: &HardeningMatrix) -> maud::Markup {
    maud::html! {
        div class="space-y-6 p-6 animate-slide-in" id="security-matrix-panel" {

            // Header Section
            div class="flex items-center justify-between border-b border-gray-800 pb-4" {
                div {
                    h2 class="text-sm font-bold font-mono text-gray-200" { "🛡️ Core Security & Hardening Matrix" }
                    p class="text-xxs font-mono text-gray-500 mt-0.5" { "Enforce engine cryptographics and inspect active HTTP request safety indicators." }
                }
                @if matrix.environment_mode == "Production" {
                    span class="px-2.5 py-1 bg-red-950/40 border border-red-800/80 text-red-400 font-mono text-xxs rounded-full font-bold" { "PRODUCTION" }
                } @else {
                    span class="px-2.5 py-1 bg-amber-950/40 border border-amber-800/80 text-amber-400 font-mono text-xxs rounded-full font-bold" { "DEVELOPMENT" }
                }
            }

            // Real-Time Request Security Header Status Board
            div class="p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-3" {
                span class="text-xxs font-mono uppercase text-gray-400 tracking-wider block" { "Active HTTP Shield Matrix" }

                div class="grid grid-cols-2 md:grid-cols-4 gap-3 text-xxs font-mono" {
                    // Content Security Policy
                    div class=(format!("p-2.5 border rounded-lg flex items-center justify-between {}", if matrix.csp_enabled { "bg-emerald-950/10 border-emerald-900/60 text-emerald-400" } else { "bg-red-950/10 border-red-900/60 text-red-400" })) {
                        span { "CSP Shield" }
                        span class="font-bold font-sans" { @if matrix.csp_enabled { "✓ ACTIVE" } @else { "✗ MISSING" } }
                    }

                    // Anti-Sniffing (X-Content-Type-Options)
                    div class=(format!("p-2.5 border rounded-lg flex items-center justify-between {}", if matrix.nosniff_enabled { "bg-emerald-950/10 border-emerald-900/60 text-emerald-400" } else { "bg-red-950/10 border-red-900/60 text-red-400" })) {
                        span { "No-Sniff" }
                        span class="font-bold font-sans" { @if matrix.nosniff_enabled { "✓ ACTIVE" } @else { "✗ MISSING" } }
                    }

                    // Clickjacking (X-Frame-Options)
                    div class=(format!("p-2.5 border rounded-lg flex items-center justify-between {}", if matrix.clickjacking_protected { "bg-emerald-950/10 border-emerald-900/60 text-emerald-400" } else { "bg-red-950/10 border-red-900/60 text-red-400" })) {
                        span { "Frame Lock" }
                        span class="font-bold font-sans" { @if matrix.clickjacking_protected { "✓ ACTIVE" } @else { "✗ MISSING" } }
                    }

                    // Strict Transport (HSTS)
                    div class=(format!("p-2.5 border rounded-lg flex items-center justify-between {}", if matrix.hsts_enabled { "bg-emerald-950/10 border-emerald-900/60 text-emerald-400" } else { "bg-amber-950/10 border-amber-900/60 text-amber-400" })) {
                        span { "HSTS Strict" }
                        span class="font-bold font-sans" { @if matrix.hsts_enabled { "✓ ENFORCED" } @else { "⚠️ OPTIONAL" } }
                    }
                }
            }

            // --- INBOUND COOKIE JAR AUDIT ---
            div class="p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-3" {
                div class="flex justify-between items-center" {
                    div {
                        span class="text-xxs font-mono uppercase text-gray-400 tracking-wider block font-semibold" { "Request Cookie Jar Sandbox Inspector" }
                        p class="text-[10px] font-mono text-gray-500 mt-0.5 leading-normal" {
                            "💡 Technical Info: Browsers omit security flags on inbound requests. Matrix confirms validation status by testing names and cryptographics."
                        }
                    }
                    span class="font-mono text-xxs px-2 py-0.5 bg-gray-900 border border-gray-800 rounded text-gray-400" {
                        (format!("{} Cookie(s) Detected", matrix.incoming_cookies.len()))
                    }
                }

                @if matrix.incoming_cookies.is_empty() {
                    div class="p-4 bg-gray-900/20 border border-dashed border-gray-800 text-center rounded-lg text-xxs font-mono text-gray-500" {
                        "No cookies passed with the current request handshake headers."
                    }
                } @else {
                    div class="overflow-hidden border border-gray-850 rounded-lg" {
                        table class="w-full text-left font-mono text-xxs m-0 border-collapse" {
                            thead class="bg-gray-900 text-gray-400 uppercase tracking-wider" {
                                tr {
                                    th class="p-2.5 font-semibold" { "Cookie Key Name" }
                                    th class="p-2.5 font-semibold" { "Value Payload Token" }
                                    th class="p-2.5 font-semibold text-right" { "Compliance Mapping" }
                                }
                            }
                            tbody class="divide-y divide-gray-850" {
                                @for cookie in &matrix.incoming_cookies {
                                    tr class="hover:bg-gray-900/40 transition" {
                                        td class="p-2.5 font-bold text-gray-300" { (cookie.name) }
                                        td class="p-2.5 text-gray-400 font-mono" { (cookie.value_preview) }
                                        td class="p-2.5 text-right" {
                                            @if cookie.server_policy_compliance == "Secure Compliant" {
                                                span class="text-emerald-400 bg-emerald-950/30 px-1.5 py-0.5 rounded border border-emerald-900/50 font-bold text-[10px]" { "✓ SECURE COMPLIANT" }
                                            } @else {
                                                span class="text-amber-400 bg-amber-950/30 px-1.5 py-0.5 rounded border border-amber-900/50 font-bold text-[10px]" { "⚠️ SECURE_PREFIX MISSING" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Grid Track Controls
            div class="grid grid-cols-1 md:grid-cols-2 gap-4" {

                // Live Sessions Lifecycle Control Vector
                div class="p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-4 flex flex-col justify-between" {
                    div class="space-y-1" {
                        div class="flex justify-between items-center" {
                            span class="text-xxs font-mono uppercase text-gray-400 tracking-wider" { "Active User Handshakes" }
                            span class="text-xs font-mono text-blue-400 font-bold" { (format!("{} Active Connections", matrix.active_admin_sessions)) }
                        }
                        p class="text-[10px] font-mono text-gray-500 leading-normal" {
                            "Track authenticated admin scopes currently alive inside the state cache."
                        }
                    }

                    button hx-post="/admin/api/security/invalidate-sessions"
                            hx-target="this"
                            hx-swap="none"
                            class="w-full bg-gray-900 border border-gray-800 hover:border-red-900/60 hover:text-red-400 text-gray-400 font-mono text-xxs py-2 rounded-lg transition text-center shadow-inner" {
                                "🛑 Terminate All Concurrent Sessions"
                    }
                }

                // Bruteforce Rate Limiter Policy
                div class="p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-4 flex flex-col justify-between" {
                    div class="space-y-1" {
                        div class="flex justify-between items-center" {
                            span class="text-xxs font-mono uppercase text-gray-400 tracking-wider" { "Bruteforce Threshold" }
                            span class="text-xs font-mono text-emerald-400 font-bold" { (format!("Max {} Attempts / IP", matrix.max_login_attempts)) }
                        }
                        p class="text-[10px] font-mono text-gray-500 leading-normal" {
                            "Rate limits requests on authentication paths before flagging temporary IP bans."
                        }
                    }

                    div class="flex gap-2" {
                        button hx-post="/admin/api/security/toggle-strict-rate-limit"
                                hx-swap="none"
                                class="flex-1 bg-emerald-950/20 border border-emerald-800/40 hover:bg-emerald-900/20 text-emerald-400 font-mono text-xxs py-2 rounded-lg transition" {
                                    "Enable Strict (3)"
                        }
                    }
                }

                // Storage Engine Encryption Cryptographics
                div class="p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-2" {
                    span class="text-xxs font-mono uppercase text-gray-400 tracking-wider block" { "Database Cryptography Layer" }
                    div class="flex items-center gap-2 pt-1" {
                        @if matrix.database_encryption_status != "Unencrypted" {
                            div class="w-2 h-2 rounded-full bg-emerald-500" {}
                            span class="text-xs font-mono text-gray-300 font-bold" { (matrix.database_encryption_status) }
                        } @else {
                            div class="w-2 h-2 rounded-full bg-red-500 animate-pulse" {}
                            span class="text-xs font-mono text-red-400 font-bold" { "At-Rest Volumes Unencrypted" }
                        }
                    }
                    p class="text-[10px] font-mono text-gray-500 pt-1 leading-normal" {
                        "Confirms if underlying storage drivers are intercepting transparent row blocks with hardware accelerated AES cryptography routines."
                    }
                }

                // TLS/SSL Transport Layer Verification
                div class="p-4 bg-gray-950 border border-gray-800 rounded-xl space-y-2" {
                    span class="text-xxs font-mono uppercase text-gray-400 tracking-wider block" { "Transport Layer Status (TLS)" }
                    div class="flex items-center gap-2 pt-1" {
                        @if matrix.ssl_active {
                            div class="w-2 h-2 rounded-full bg-emerald-500" {}
                            span class="text-xs font-mono text-gray-300 font-bold" { "HTTPS Strict Connection Verified" }
                        } @else {
                            div class="w-2 h-2 rounded-full bg-amber-500" {}
                            span class="text-xs font-mono text-amber-500 font-bold" { "HTTP Plaintext Unsafe Vector" }
                        }
                    }
                    p class="text-[10px] font-mono text-gray-500 pt-1 leading-normal" {
                        "Verifies whether client request handshakes are running inside secure encrypted wrappers before parsing application middleware inputs."
                    }
                }
            }
        }
    }
}
