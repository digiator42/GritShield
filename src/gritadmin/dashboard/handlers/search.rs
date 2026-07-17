use crate::database::repository::CustomQuerySpec;
use crate::database::repository::GritRepository;
use crate::database::repository::registry::ADMIN_REGISTRY;
use crate::database::repository::registry::AdminHandlerFn;
use crate::database::repository::JqlCompiler;
use crate::gritadmin::dashboard::{render_grid_rows, render_results_grid, render_empty_matrix_interface, error_response};
use crate::security::xss::UntrustedString;
use crate::prelude::*;
use maud::html;
use sea_orm::QueryOrder;
use sea_orm::EntityTrait;
use sea_orm::sea_query::{Alias, ColumnDef, Table};
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::QueryResult;
use std::collections::HashMap;
use sea_orm::ConnectionTrait;

/// Generic search query processor handling dynamic query filters with inline drop capabilities.
pub async fn handle_search<R>(ctx: RequestContext, repo: R, table_slug: &'static str) -> Response
where
    R: GritRepository + Send + Sync + 'static,
    <R as GritRepository>::Model: Sync + Send,
{
    let db = repo.get_db();
    let query = ctx
        .query
        .get("q")
        .map(|v| v.to_string())
        .unwrap_or_default();

    let items = if query.is_empty() {
        <R::Entity as EntityTrait>::find()
            .order_by_desc(R::id_column())
            .all(db)
            .await
            .unwrap_or_default()
    } else {
        repo.search_admin_fields(&query).await.unwrap_or_default()
    };

    // Same row template as the main matrix (checkbox, FK links, inline edit, view/delete
    // icons) — previously this duplicated an older, slightly different template that was
    // missing the checkbox column and FK linking, so bulk-select and "jump to related
    // record" silently didn't work when rows came from quick search.
    let rows_html = render_grid_rows(&repo, &items, table_slug, true);
    Response::ok(rows_html.into_string())
}


/// Global command palette search engine covering dynamic tables, settings, and falling back to deep record matching.
pub async fn handle_search_palette(ctx: RequestContext) -> Response {
    let query = ctx
        .query
        .get("q")
        .map(|v| v.to_string().to_lowercase())
        .unwrap_or_default();

    let static_settings = vec![
        ("/admin/metrics", "⚙️ System Metrics"),
        ("/admin/settings/security", "🛡️ Hardening Matrix"),
    ];

    let (filtered_tables, filtered_settings, search_targets) = {
        let registry = ADMIN_REGISTRY.lock().unwrap();

        // 1. Fully own the matched table entries as cloned Strings so we don't borrow from registry
        let tables: Vec<(String, String)> = registry
            .iter()
            .filter(|(table_name, _)| {
                !query.is_empty() && table_name.to_lowercase().contains(&query)
            })
            .map(|(table_name, meta)| (table_name.to_string(), meta.route_path.to_string()))
            .collect();

        // 2. Filter settings
        let settings: Vec<(String, String)> = static_settings
            .into_iter()
            .filter(|(path, label)| {
                !query.is_empty()
                    && (label.to_lowercase().contains(&query)
                        || path.to_lowercase().contains(&query))
            })
            .map(|(path, label)| (path.to_string(), label.to_string()))
            .collect();

        // 3. Clone the handler pointers for fallback routing
        let targets: Vec<(String, String, AdminHandlerFn)> = registry
            .iter()
            .map(|(table_name, meta)| {
                (
                    table_name.to_string(),
                    meta.route_path.to_string(),
                    meta.search_handler.clone(),
                )
            })
            .collect();

        (tables, settings, targets)
    };

    let mut record_results: Vec<(String, String, String)> = Vec::new();

    // Now, running async operations here is 100% thread-safe
    if filtered_tables.is_empty() && filtered_settings.is_empty() && !query.is_empty() {
        for (table_name, route_path, search_handler) in search_targets {
            let mut sub_ctx = ctx.clone();
            sub_ctx
                .query
                .insert("q".to_string(), UntrustedString::new(query.clone()));

            let search_response = search_handler(sub_ctx).await;

            if search_response.status == 200 {
                let (body_bytes, _) = search_response.resolve();
                let html_body = String::from_utf8(body_bytes).unwrap_or_default();

                if !html_body.trim().is_empty() {
                    record_results.push((table_name, route_path, html_body));
                }
            }
        }
    }

    // Render the final UI using our isolated variables
    let results_html = html! {
        @if filtered_tables.is_empty() && filtered_settings.is_empty() && record_results.is_empty() {
            div class="p-4 text-center text-gray-500 font-mono text-xs" {
                "No workspace parameters match your query matrix."
            }
        } @else {
            @if !filtered_tables.is_empty() {
                @for (table_name, route_path) in filtered_tables {
                    @let display_name = format!(
                        "{} Grid",
                        table_name
                            .chars()
                            .next()
                            .map(|c| c.to_uppercase().to_string())
                            .unwrap_or_default()
                            + &table_name[1..]
                    );
                    a href=(route_path)
                       hx-get=(route_path)
                       hx-target="#main-content"
                       hx-push-url="true"
                       onclick="document.getElementById('command-palette').classList.add('hidden')"
                       class="flex items-center justify-between p-3 rounded-lg hover:bg-gray-800/60 transition group font-medium" {
                           span { (display_name) }
                           span class="text-xs text-gray-500 font-mono opacity-0 group-hover:opacity-100 transition" { (route_path) }
                    }
                }
            }

            @if !filtered_settings.is_empty() {
                hr {}
                @for (path, label) in filtered_settings {
                    a href=(path)
                       hx-get=(path)
                       hx-target="#main-content"
                       hx-push-url="true"
                       onclick="document.getElementById('command-palette').classList.add('hidden')"
                       class="flex items-center justify-between p-3 rounded-lg hover:bg-gray-800/60 transition group font-medium" {
                           span { (label) }
                           span class="text-xs text-gray-500 font-mono opacity-0 group-hover:opacity-100 transition" { (path) }
                    }
                }
            }

            @if !record_results.is_empty() {
                div class="text-xxs uppercase tracking-wider text-amber-500 font-bold px-3 py-2 font-mono" { "🔍 Deep Record Matches" }
                div class="max-h-72 overflow-y-auto space-y-4 px-2 py-1" {
                    @for (table_slug, route_path, rows_snippet) in &mut record_results {
                        div class="border border-gray-900 bg-gray-950/40 rounded-lg overflow-hidden" {
                            div class="bg-gray-900/60 px-3 py-1.5 flex justify-between items-center border-b border-gray-900" {
                                span class="text-xs font-bold text-gray-400 uppercase tracking-tight" { (table_slug) }
                                a href=(route_path)
                                   hx-get=(route_path)
                                   hx-target="#main-content"
                                   hx-push-url="true"
                                   onclick="document.getElementById('command-palette').classList.add('hidden')"
                                   class="text-xxs text-emerald-500 hover:underline font-mono" { "Go →" }
                            }
                            table class="w-full text-left border-collapse pointer-events-none select-none opacity-85 table-scroll" {
                                @let max_len = rows_snippet.len().min(10);
                                tbody class="divide-y divide-gray-900/60 bg-gray-950/20" {
                                    (maud::PreEscaped(&rows_snippet[..max_len]))
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    Response::ok(results_html.into_string())
}

/// Unified asynchronous query execution engine matching web interface requests
pub async fn handle_custom_search_viewer<R>(
    ctx: RequestContext,
    repo: R,
    table_slug: &'static str,
) -> Response
where
    R: GritRepository + Send + Sync + 'static,
    <R as GritRepository>::Model: Sync + Send,
    <R as GritRepository>::Id: std::str::FromStr,
    <<R as GritRepository>::Id as std::str::FromStr>::Err: std::fmt::Display,
{
    let query_input = ctx.query.get("jql").map(|v| v.as_str()).unwrap_or("");
    let db = ctx.db.clone().expect("DB connection missing");

    if query_input.is_empty() {
        return Response::ok(render_empty_matrix_interface().into_string());
    }

    // Attempt to evaluate user expression via string token rules engine
    let parsed_spec = match CustomQuerySpec::parse_from_str(query_input) {
        Ok(spec) => spec,
        Err(err) => {
            return Response::ok(html! {
                div id="matrix-wrapper" class="bg-red-950/20 border border-red-900/50 rounded-xl p-4 font-mono text-xs text-red-400" {
                    span class="font-bold uppercase block mb-1" { "⚠️ Syntax Mapping Error:" }
                    (err)
                }
            }.into_string());
        }
    };

    // Automatically resolve whether the active runtime target is Postgres, MySQL, or SQLite
    let db_backend = db.get_database_backend();
    let native_stmt = JqlCompiler::compile(&parsed_spec, db_backend);

    match db.query_all(native_stmt).await {
        Ok(query_results) => {
            if query_results.is_empty() {
                return Response::ok(html! {
                    div id="matrix-wrapper" class="p-6 text-center text-gray-500 font-mono text-xs border border-gray-800 rounded-xl" {
                        "Query completed successfully. Execution returned an empty zero-row dataset."
                    }
                }.into_string());
            }

            // Dynamically discover what columns came back inside the generic payload matrix
            let column_headers: Vec<String> = parsed_spec
                .select_columns
                .iter()
                .map(|(tbl, col)| {
                    tbl.as_ref()
                        .map_or(col.clone(), |t| format!("{}.{}", t, col))
                })
                .collect();

            // Pass down &repo into the grid to draw editable fields matching your layout rules
            let rendered_view = render_results_grid(&column_headers, &query_results, table_slug, &repo);
            Response::ok(rendered_view.into_string())
        }
        Err(db_err) => Response::ok(html! {
            div id="matrix-wrapper" class="bg-red-950/20 border border-red-900/50 rounded-xl p-4 font-mono text-xs text-red-400" {
                span class="font-bold uppercase block mb-1" { "⚙️ Database Engine Refusal:" }
                (db_err.to_string())
            }
        }.into_string())
    }
}
