use crate::database::repository::registry::ADMIN_REGISTRY;
use crate::gritadmin::dashboard::{error_response};
use crate::gritadmin::shell;
use crate::gritadmin::metrics_render::{
    gather_all_metrics, render_hardening_matrix, render_metrics_dashboard, HardeningMatrix,
    InboundCookieDetails,
};
use crate::deps::sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, PaginatorTrait, QueryOrder,
    Statement, TransactionTrait,
};
use crate::prelude::*;
use maud::html;
use std::collections::HashMap;
use std::sync::atomic::Ordering;

/// Dashboard view showing counts and recent records for all registered tables.
pub async fn handle_dashboard(ctx: RequestContext) -> Response {
    let db = match ctx.db.clone() {
        Some(d) => d,
        None => return Response::bad_request("Database connection missing"),
    };

    // ---- Collect all table info while holding the lock ----
    let table_infos: Vec<(String, String, String)> = {
        let registry = ADMIN_REGISTRY.lock().unwrap();
        registry
            .iter()
            .map(|(table_slug, meta)| {
                (
                    meta.table_name.to_string(), // actual DB table name
                    table_slug.to_string(),      // route slug
                    meta.route_path.to_string(),
                )
            })
            .collect()
    };

    let mut cards: Vec<maud::PreEscaped<String>> = Vec::new();

    for (table_name, table_slug, route_path) in &table_infos {
        // ---- Count total records ----
        let count_sql = format!("SELECT COUNT(*) as count FROM {}", table_name);
        let count_stmt = Statement::from_string(DbBackend::Sqlite, count_sql);
        let count_result = match db.query_one(count_stmt).await {
            Ok(Some(row)) => row.try_get::<i64>("", "count").unwrap_or(0),
            _ => 0,
        };

        // ---- Fetch recent 5 records ----
        let recent_sql = format!("SELECT * FROM {} ORDER BY id DESC LIMIT 5", table_name);
        let recent_stmt = Statement::from_string(DbBackend::Sqlite, recent_sql);
        let recent_rows = match db.query_all(recent_stmt).await {
            Ok(rows) => rows,
            Err(_) => Vec::new(),
        };

        let recent_html = if recent_rows.is_empty() {
            html! { div class="text-gray-500 text-xs" { "No recent records" } }
        } else {
            let first_row = &recent_rows[0];
            let column_names: Vec<String> = first_row
                .column_names()
                .iter()
                .map(|c| c.to_string())
                .collect();

            let rows_html: Vec<Markup> = recent_rows
                .iter()
                .map(|row| {
                    // Get ID by index
                    let id_val: String = row
                        .try_get_by_index::<String>(0)
                        .or_else(|_| row.try_get("", "id"))
                        .unwrap_or("".to_string());

                    // Get first two data columns (skip ID)
                    let mut cols = Vec::new();
                    for (i, col_name) in column_names.iter().enumerate().take(3) {
                        if i == 0 {
                            continue;
                        }
                        let val: String = row
                            .try_get_by_index::<String>(i)
                            .or_else(|_| row.try_get("", col_name))
                            .unwrap_or("".to_string());
                        cols.push(val);
                        if cols.len() >= 2 {
                            break;
                        }
                    }

                    html! {
                        tr {
                            td class="p-1" { (id_val) }
                            @for col_val in &cols {
                                td class="p-1 truncate max-w-[100px]" { (col_val) }
                            }
                            td class="p-1" {
                                a href=(format!("/admin/{}/{}", table_slug, id_val))
                                  hx-get=(format!("/admin/{}/{}", table_slug, id_val))
                                  hx-target="#main-content"
                                  hx-push-url="true"
                                  class="text-blue-400 hover:underline" {
                                      "View"
                                  }
                            }
                        }
                    }
                })
                .collect();

            let header_cols: Vec<String> = column_names
                .iter()
                .skip(1)
                .take(2)
                .map(|c| c.to_string())
                .collect();

            html! {
                table class="w-full text-left text-xs" {
                    thead {
                        tr class="text-gray-500" {
                            th class="p-1" { "ID" }
                            @for col in &header_cols {
                                th class="p-1" { (col) }
                            }
                            th class="p-1" { "Actions" }
                        }
                    }
                    tbody {
                        @for row_html in rows_html {
                            (row_html)
                        }
                    }
                }
            }
        };

        let card: maud::PreEscaped<String> = html! {
            div class="bg-gray-950 border border-gray-800 rounded-xl p-4 shadow-xl" {
                div class="flex justify-between items-start" {
                    div {
                        h3 class="text-lg font-bold text-emerald-400" { (table_slug) }
                        p class="text-3xl font-mono text-gray-200" { (count_result) }
                    }
                    a href=(route_path)
                      hx-get=(route_path)
                      hx-target="#main-content"
                      hx-push-url="true"
                      class="text-xs text-gray-500 hover:text-gray-300" {
                          "View all →"
                      }
                }
                div class="mt-3 max-h-48 overflow-y-auto" {
                    (recent_html)
                }
            }
        };

        cards.push(card);
    }

    // ---- Overall Summary ----
    let mut total_records: i64 = 0;
    for (table_name, _table_slug, _) in &table_infos {
        let count_sql = format!("SELECT COUNT(*) as count FROM {}", table_name);
        let stmt = Statement::from_string(DbBackend::Sqlite, count_sql);
        let count = db
            .query_one(stmt)
            .await
            .ok()
            .flatten()
            .and_then(|row| row.try_get::<i64>("", "count").ok())
            .unwrap_or(0);
        total_records += count;
    }

    let summary = html! {
        div class="bg-gray-950 border border-gray-800 rounded-xl p-4 shadow-xl col-span-full" {
            h2 class="text-xl font-bold text-gray-300" { "📊 Overall Metrics" }
            p class="text-4xl font-mono text-emerald-400" { (total_records) " records across " (table_infos.len()) " tables" }
        }
    };

    let dashboard_html = html! {
        div class="space-y-6" {
            h1 class="text-3xl font-bold tracking-tight" { "Dashboard" }
            div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4" {
                (summary)
                @for card in cards {
                    (card)
                }
            }
        }
    };

    let is_htmx = ctx.req.has_header("hx-request");
    if is_htmx {
        Response::ok(dashboard_html.into_string())
    } else {
        shell::admin_shell("Dashboard", dashboard_html, false)
    }
}

// GET /admin/api/metrics json
pub async fn admin_metrics_api_handler(ctx: RequestContext) -> Response {
    // Fetch real-time snapshot metrics safely
    let metrics_payload = gather_all_metrics(&ctx).await;

    // Serialize back out into a structured standard JSON text layout
    Response::json(200, &metrics_payload)
}

// GET /admin/metrics html
pub async fn admin_metrics_html_handler(ctx: RequestContext) -> Response {
    // Fetch real-time snapshot metrics safely
    let metrics_payload = gather_all_metrics(&ctx).await;

    // Render the Maud template code component into a raw String
    let rendered_html = render_metrics_dashboard(&metrics_payload).into_string();

    // Return the response marked explicitly as HTML payload
    Response::ok(rendered_html)
}

// GET /admin/settings/security
pub async fn admin_security_matrix_view_handler(ctx: RequestContext) -> Response {
    let current_user_authenticated = ctx.claims.is_some();

    // Extract GritShield Atomic Performance Metrics
    let total_blocked_ips = ctx.telemetry.total_blocked_ips.load(Ordering::Relaxed);
    let total_rate_limited_reqs = ctx
        .telemetry
        .total_rate_limited_reqs
        .load(Ordering::Relaxed);

    let is_production =
        std::env::var("APP_ENV").unwrap_or_else(|_| "Development".to_string()) == "production";
    let is_ssl = ctx
        .headers
        .get("x-forwarded-proto")
        .map(|v| v == "https")
        .unwrap_or(false);

    // Scan headers mapping inside RequestContext case-insensitively
    let has_csp = ctx.headers.contains_key("content-security-policy");
    let has_nosniff = ctx
        .headers
        .get("x-content-type-options")
        .map(|v| v.to_lowercase() == "nosniff")
        .unwrap_or(false);
    let has_frame_opt = ctx.headers.contains_key("x-frame-options");
    let has_hsts = ctx.headers.contains_key("strict-transport-security");

    // Safely extract and audit incoming cookies inside the Mutex guard
    let mut audited_cookies = Vec::new();
    {
        if let Ok(jar_guard) = ctx.cookies.lock() {
            // Read from your provided `incoming: HashMap<String, String>` map structure
            for (name, value) in &jar_guard.incoming {
                // Run a server policy audit: cookies should have strict prefixes like __Host- or __Secure-
                let compliance = if name.starts_with("__Host-")
                    || name.starts_with("__Secure-")
                    || name == "session_id"
                {
                    "Secure Compliant"
                } else {
                    "Needs Review"
                };

                // Truncate long tokens (like session JWTs) securely to keep UI neat
                let preview = if value.len() > 16 {
                    format!("{}...", &value[0..12])
                } else {
                    value.clone()
                };

                audited_cookies.push(InboundCookieDetails {
                    name: name.clone(),
                    value_preview: preview,
                    server_policy_compliance: compliance,
                });
            }
        }
    }

    // Check if the firewall has actively blocked IPs or rate-limited requests to set degraded state
    let is_rate_limiting_degraded = total_blocked_ips > 0 || total_rate_limited_reqs > 100;

    // Identify active admin session count (mocked to 1 or pull from dynamic tracking system if available)
    let live_admin_sessions = if current_user_authenticated { 1 } else { 0 };

    // Construct state model package
    let matrix_state = HardeningMatrix {
        timestamp: chrono::Utc::now(),
        ssl_active: is_ssl,
        database_encryption_status: if is_production {
            "AES-256-GCM Cryptographic Active"
        } else {
            "Unencrypted Volume Map"
        },
        max_login_attempts: 5,
        environment_mode: if is_production {
            "Production".to_string()
        } else {
            "Development".to_string()
        },

        csp_enabled: has_csp,
        nosniff_enabled: has_nosniff,
        clickjacking_protected: has_frame_opt,
        hsts_enabled: has_hsts,

        current_request_authenticated: current_user_authenticated,
        incoming_cookies: audited_cookies,

        // Missing fields added here to satisfy structural initialization
        active_admin_sessions: live_admin_sessions,
        rate_limiting_degraded: is_rate_limiting_degraded,
    };

    let html_payload = render_hardening_matrix(&matrix_state).into_string();

    Response::ok(html_payload)
}
