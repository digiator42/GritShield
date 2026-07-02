use crate::database::repository::AdminHandlerFn;
use crate::database::repository::{CustomQuerySpec, JoinSpec, JqlCompiler, WhereSpec};
use crate::database::repository::{GritRepository, ADMIN_REGISTRY};
use crate::deps::sea_orm::{
    ConnectionTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryOrder, TransactionTrait,
};
use crate::security::errors::ShieldError;
use crate::security::xss::UntrustedString;
use crate::{admin_shell, prelude::*};
use maud::html;
use sea_orm::QueryResult;

/// Generic dashboard view runner for listing data rows and handling infinite scrolls.
pub async fn handle_list<R>(ctx: RequestContext, repo: R, table_slug: &'static str) -> Response
where
    R: GritRepository + Send + Sync + 'static,
    <R as GritRepository>::Model: Sync + Send,
    <R as GritRepository>::Id: std::str::FromStr,
    <<R as GritRepository>::Id as std::str::FromStr>::Err: std::fmt::Display,
{
    let is_htmx = ctx.req.has_header("hx-request");
    let db = repo.get_db();

    // ---- Sorting ----
    let sort_col = ctx.query.get("sort").map(|v| v.as_str()).unwrap_or("");
    let sort_dir = ctx
        .query
        .get("direction")
        .map(|v| v.as_str())
        .unwrap_or("desc");
    let mut query = <R::Entity as EntityTrait>::find();

    if let Some(col) = repo.column_from_str(sort_col) {
        if sort_dir == "asc" {
            query = query.order_by_asc(col);
        } else {
            query = query.order_by_desc(col);
        }
    } else {
        // default order by id desc
        query = query.order_by_desc(R::id_column());
    }

    // ---- Pagination ----
    let page = ctx
        .query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let is_infinite_scroll = is_htmx && page > 0;
    let page_size = 15;

    let paginator = query.paginate(db, page_size);
    let total_pages = paginator.num_pages().await.unwrap_or(0);
    let items = paginator.fetch_page(page).await.unwrap_or_default();

    let route_path_str = format!("/admin/{}", table_slug);
    let route_patch_str = format!("/admin/{}/update-cell", table_slug);
    let route_delete_str = format!("/admin/{}/delete", table_slug);
    let route_advanced_str = format!("/admin/{}/query-explorer", table_slug);
    let route_detail_str = format!("/admin/{}/", table_slug); // used with id appended
    let route_bulk_delete_str = format!("/admin/{}/bulk-delete", table_slug);

    // Build the sort link helper to preserve current sort parameters
    let sort_link = |col: &str| {
        let new_dir = if sort_col == col && sort_dir == "asc" {
            "desc"
        } else {
            "asc"
        };
        format!("{}?sort={}&direction={}", route_path_str, col, new_dir)
    };

    let rows_html = html! {
        @for item in items.iter() {
            @let record_id = repo.get_field_as_string(item, "id");
            tr class="divide-x divide-gray-800 hover:bg-gray-900/40 transition group" {
                // ---- Checkbox column ----
                td class="p-3 text-center w-10" {
                    input type="checkbox"
                        name="selected_ids"
                        value=(record_id)
                        class="form-checkbox bg-gray-800 border-gray-700 rounded text-emerald-500 focus:ring-0 focus:ring-offset-0";
                }
                @for col in repo.grid_columns().iter() {
                    td class="p-3 text-sm font-medium" {
                        @if col.is_editable {
                            input type="text"
                                value=(repo.get_field_as_string(item, &col.name))
                                name=(col.name)
                                hx-patch=(route_patch_str)
                                hx-trigger="change, keyup[key=='Enter']"
                                hx-target="this"
                                hx-swap="outerHTML"
                                hx-vals=(format!("{{\"id\": \"{}\", \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", record_id, col.name, table_slug))
                                class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
                        } @else {
                            span class="px-2 py-1 text-gray-400 font-mono text-xs" { (repo.get_field_as_string(item, &col.name)) }
                        }
                    }
                }
                // ---- Action column ----
                td class="p-3 text-center w-24" {
                    // Detail link (eye icon)
                    a href=(format!("{}{}", route_detail_str, record_id))
                      hx-get=(format!("{}{}", route_detail_str, record_id))
                      hx-target="#main-content"
                      hx-push-url="true"
                      class="text-blue-400/60 hover:text-blue-400 p-1 rounded hover:bg-blue-950/30 font-mono text-xs transition duration-150 mr-2" {
                        "👁"
                    }
                    // Delete button
                    button
                        hx-delete=(route_delete_str)
                        hx-vals=(format!("{{\"id\": \"{}\"}}", record_id))
                        hx-target="closest tr"
                        hx-swap="outerHTML"
                        hx-confirm="Are you sure you want to permanently delete this record?"
                        class="text-red-500/60 hover:text-red-400 p-1 rounded hover:bg-red-950/30 font-mono text-xs transition duration-150" {
                            "✕"
                    }
                }
            }
        }
        @if (page + 1) < total_pages {
            tr id="infinite-scroll-spinner"
                hx-get=(format!("{}?page={}&sort={}&direction={}", route_path_str, page + 1, sort_col, sort_dir))
                hx-trigger="intersect once"
                hx-target="#infinite-scroll-spinner"
                hx-swap="outerHTML"
                class="border-t border-gray-900 bg-gray-950/50 animate-pulse" {
                td colspan=(&(repo.grid_columns().len() + 2)) class="p-4 text-center" {
                    span class="text-xs text-gray-400 font-medium" { "Loading more records..." }
                }
            }
        }
    };

    if is_infinite_scroll {
        // Infinite scroll: return only the new rows (as a fragment)
        Response::ok(rows_html.into_string())
    } else if is_htmx {
        // HTMX request (sort, search, or initial load with HTMX): return only the matrix-wrapper
        let matrix_html = html! {
            div id="matrix-wrapper" class="bg-gray-950 border border-gray-800 rounded-xl overflow-hidden shadow-xl" {
                table class="w-full text-left border-collapse" {
                    thead class="bg-gray-900/80 border-b border-gray-800 text-xs font-semibold uppercase tracking-wider text-gray-400" {
                        tr class="divide-x divide-gray-800" {
                            th class="p-4 text-center w-10" { "" } // checkbox header
                            @for col in repo.grid_columns().iter() {
                                th class="p-4" {
                                    a href=(sort_link(&col.name))
                                       hx-get=(sort_link(&col.name))
                                       hx-target="#matrix-wrapper"
                                       hx-swap="outerHTML"
                                       class="hover:text-white transition flex items-center gap-1" {
                                           (col.label)
                                           @if sort_col == col.name {
                                               @if sort_dir == "asc" { "↑" } @else { "↓" }
                                           }
                                       }
                                }
                            }
                            th class="p-4 text-center w-24" { "Actions" }
                        }
                    }
                    tbody id="table-body" class="divide-y divide-gray-800" { (rows_html) }
                }
                // Bulk actions footer (same as before)
                div class="bg-gray-900/80 border-t border-gray-800 px-4 py-3 flex items-center justify-between" {
                    div class="flex items-center gap-4" {
                        span class="text-xs text-gray-400" {
                            "Selected: "
                            span id="selected-count" class="font-mono text-emerald-400" { "0" }
                        }
                        button
                            hx-post=(route_bulk_delete_str)
                            hx-vals="ids=[]"
                            hx-include="[name='selected_ids']"
                            hx-target="#matrix-wrapper"
                            hx-swap="outerHTML"
                            hx-confirm="Delete all selected records?"
                            class="bg-red-950/40 hover:bg-red-900/40 text-red-400 text-xs font-mono font-semibold px-4 py-2 rounded-lg transition duration-150 disabled:opacity-50 disabled:cursor-not-allowed"
                            id="bulk-delete-btn"
                            disabled {
                                "Delete Selected"
                        }
                    }
                    span class="text-xxs text-gray-500 font-mono" {
                        "Click headers to sort"
                    }
                }
            }
        };
        Response::ok(matrix_html.into_string())
    } else {
        let display_title = format!("{} Workspace Matrix", table_slug.to_uppercase());
        let route_search_str = format!("/admin/{}/search", table_slug);

        // Full page with header, filter, table, and bulk action footer
        let complete_view = html! {
            div class="space-y-6" {
                div class="bg-gray-950 border border-gray-800 rounded-xl p-4 shadow-xl space-y-3" {
                    div class="flex items-center justify-between" {
                        h2 class="text-xs font-bold tracking-wider text-emerald-500 uppercase font-mono" {
                            "Matrix Query Explorer JQL"
                        }
                        span class="text-xxs font-mono text-gray-500" { "Supports SELECT ... FROM ... JOIN ... WHERE ..." }
                    }
                    div class="flex gap-2" {
                        input type="text"
                            name="jql"
                            placeholder="select id,title from projects join assignments on projects.id = assignments.project_id where status = 'active'"
                            hx-get=(route_advanced_str)
                            hx-trigger="keyup[key=='Enter']"
                            hx-target="#matrix-wrapper"
                            hx-swap="outerHTML"
                            class="bg-gray-900 border border-gray-800 rounded-lg px-4 py-2.5 flex-1 text-xs font-mono text-emerald-400 focus:outline-none focus:border-emerald-500 placeholder-gray-600 transition shadow-inner";

                        button
                            hx-get=(route_advanced_str)
                            hx-include="[name='jql']"
                            hx-target="#matrix-wrapper"
                            hx-swap="outerHTML"
                            class="bg-emerald-950/40 border border-emerald-800/60 hover:bg-emerald-900/40 text-emerald-400 text-xs font-mono font-semibold px-4 py-2 rounded-lg transition duration-150" {
                                "Run Query"
                        }
                    }
                }


                div class="flex justify-between items-center" {
                    h1 class="text-2xl font-bold tracking-tight" { (display_title) }
                    input type="text"
                        name="q"
                        placeholder="Basic keyword lookup..."
                        hx-get=(route_search_str)
                        hx-trigger="keyup changed delay:300ms"
                        hx-target="#table-body"
                        class="bg-gray-950 border border-gray-800 rounded-lg px-4 py-2 w-80 text-sm focus:outline-none focus:border-emerald-500 transition";
                }

                div id="matrix-wrapper" class="bg-gray-950 border border-gray-800 rounded-xl overflow-hidden shadow-xl" {
                    table class="w-full text-left border-collapse" {
                        thead class="bg-gray-900/80 border-b border-gray-800 text-xs font-semibold uppercase tracking-wider text-gray-400" {
                            tr class="divide-x divide-gray-800" {
                                th class="p-4 text-center w-10" { "" } // checkbox header
                                @for col in repo.grid_columns().iter() {
                                    th class="p-4" {
                                        a href=(sort_link(&col.name))
                                           hx-get=(sort_link(&col.name))
                                           hx-target="#matrix-wrapper"
                                           hx-swap="outerHTML"
                                           class="hover:text-white transition flex items-center gap-1" {
                                               (col.label)
                                               @if sort_col == col.name {
                                                   @if sort_dir == "asc" { "↑" } @else { "↓" }
                                               }
                                           }
                                    }
                                }
                                th class="p-4 text-center w-24" { "Actions" }
                            }
                        }
                        tbody id="table-body" class="divide-y divide-gray-800" { (rows_html) }
                    }
                    // ---- Bulk actions footer ----
                    div class="bg-gray-900/80 border-t border-gray-800 px-4 py-3 flex items-center justify-between" {
                        div class="flex items-center gap-4" {
                            span class="text-xs text-gray-400" {
                                "Selected: "
                                span id="selected-count" class="font-mono text-emerald-400" { "0" }
                            }
                            button
                                hx-post=(route_bulk_delete_str)
                                hx-vals="ids=[]" // will be updated by JavaScript
                                hx-include="[name='selected_ids']"
                                hx-target="#matrix-wrapper"
                                hx-swap="outerHTML"
                                hx-confirm="Delete all selected records?"
                                class="bg-red-950/40 hover:bg-red-900/40 text-red-400 text-xs font-mono font-semibold px-4 py-2 rounded-lg transition duration-150 disabled:opacity-50 disabled:cursor-not-allowed"
                                id="bulk-delete-btn"
                                disabled {
                                    "Delete Selected"
                            }
                        }
                        span class="text-xxs text-gray-500 font-mono" {
                            "Click headers to sort"
                        }
                    }
                }
            }
        };
        admin_shell(&display_title, complete_view, is_htmx)
    }
}

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

    let route_patch_str = format!("/admin/{}/update-cell", table_slug);
    let route_delete_str = format!("/admin/{}/delete", table_slug);

    let rows_html = html! {
        @for item in items.iter() {
            @let record_id = repo.get_field_as_string(item, "id");
            tr class="divide-x divide-gray-800 hover:bg-gray-900/40 transition group" {
                @for col in repo.grid_columns().iter() {
                    td class="p-3 text-sm font-medium" {
                        @if col.is_editable {
                            input type="text"
                                value=(repo.get_field_as_string(item, &col.name))
                                name=(col.name)
                                hx-patch=(route_patch_str)
                                hx-trigger="change"
                                hx-target="this"
                                hx-swap="outerHTML"
                                hx-vals=(format!("{{\"id\": \"{}\", \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", record_id, col.name, table_slug))
                                class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
                        } @else {
                            span class="px-2 py-1 text-gray-400 font-mono text-xs" { (repo.get_field_as_string(item, &col.name)) }
                        }
                    }
                }
                td class="p-3 text-center w-20" {
                    button
                        hx-delete=(route_delete_str)
                        hx-vals=(format!("{{\"id\": \"{}\"}}", record_id))
                        hx-target="closest tr"
                        hx-swap="outerHTML"
                        hx-confirm="Are you sure you want to permanently delete this record?"
                        class="text-red-500/60 hover:text-red-400 p-1 rounded hover:bg-red-950/30 font-mono text-xs opacity-0 group-hover:opacity-100 transition duration-150" {
                            "Drop"
                    }
                }
            }
        }
    };
    Response::ok(rows_html.into_string())
}

/// Generic database record removal handler matching HTMX asynchronous delete operations.
pub async fn handle_delete<R>(ctx: RequestContext, repo: R, _table_slug: &'static str) -> Response
where
    R: GritRepository + Send + Sync + 'static,
    <R as GritRepository>::Model: Sync + Send,
    <R as GritRepository>::Id: std::str::FromStr,
    <<R as GritRepository>::Id as std::str::FromStr>::Err: std::fmt::Display,
{
    let record_id_raw = match ctx
        .form
        .fields
        .get("id")
        .map(|v| v.as_str())
        .or_else(|| ctx.query.get("id").map(|v| v.as_str()))
    {
        Some(id) => id,
        None => return error_response("Missing record target ID"),
    };

    let record_id = match record_id_raw.parse::<<R as GritRepository>::Id>() {
        Ok(id) => id,
        Err(e) => return error_response(format!("Invalid record ID: {}", e)),
    };

    match repo.delete_by_id(record_id).await {
        Ok(_) => Response::ok(""),
        Err(e) => error_response(format!("Database removal failed: {}", e)),
    }
}

/// Generic column/cell PATCH updates with standard validation pipelines.
pub async fn handle_patch<R>(ctx: RequestContext, repo: R, table_slug: &'static str) -> Response
where
    R: GritRepository + Send + Sync + 'static,
    <R as GritRepository>::Model: Sync + Send,
    <R as GritRepository>::Id: std::str::FromStr,
    <<R as GritRepository>::Id as std::str::FromStr>::Err: std::fmt::Display,
{
    let form = ctx.form.fields;
    let record_id_raw = match form.get("id") {
        Some(id) => id,
        None => return error_response("Missing record ID"),
    };
    let record_id = match record_id_raw.parse::<<R as GritRepository>::Id>() {
        Ok(id) => id,
        Err(e) => return error_response(format!("Invalid record ID: {}", e)),
    };
    let raw_column = match form.get("column") {
        Some(col) => col.as_str(),
        None => return error_response("Missing targeted column"),
    };
    let column_name = Sanitizer::url_decode(raw_column);
    let raw_value = match form.get(raw_column) {
        Some(val) => val.as_str(),
        None => return error_response("Missing field update payload"),
    };
    let target_value = Sanitizer::url_decode(raw_value);

    match repo
        .update_column_value(record_id, &column_name, target_value)
        .await
    {
        Ok(updated_model) => {
            let display_value = repo.get_field_as_string(&updated_model, &column_name);
            let single_input_html = html! {
                input type="text"
                    value=(display_value)
                    hx-patch=(format!("/admin/{}/update-cell", table_slug))
                    hx-trigger="change, keyup[key=='Enter']"
                    name=(column_name)
                    hx-target="this"
                    hx-swap="outerHTML"
                    hx-vals=(format!("{{\"id\": \"{}\", \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", record_id_raw, column_name, table_slug))
                    class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
            };
            Response::ok(single_input_html.into_string())
        }
        Err(e) => error_response(format!("Database field rejection: {}", e)),
    }
}

/// Global command palette search engine covering dynamic tables, settings, and falling back to deep record matching.
pub async fn handle_search_palette(ctx: RequestContext) -> Response {
    let query = ctx
        .query
        .get("q")
        .map(|v| v.to_string().to_lowercase())
        .unwrap_or_default();

    let static_settings = vec![
        ("/admin/settings", "⚙️ System Metrics"),
        ("/admin/settings/security", "🛡️ Hardening Matrix"),
        ("/admin/settings/logs", "📜 Audit Log"),
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

        println!("======>> {:?}", settings);

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
                // div class="text-xxs uppercase tracking-wider text-emerald-500 font-bold px-3 py-2 font-mono" { "📂 Table Workspaces" }
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
                // div class="text-xxs uppercase tracking-wider text-blue-400 text-[0.5rem] px-3 py-2 font-mono" { "System Control" }
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
                    @for (table_slug, route_path, rows_snippet) in record_results {
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
                            table class="w-full text-left border-collapse pointer-events-none select-none opacity-85" {
                                tbody class="divide-y divide-gray-900/60 bg-gray-950/20" {
                                    (maud::PreEscaped(rows_snippet))
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
pub async fn handle_custom_search_viewer(ctx: RequestContext) -> Response {
    let query_input = ctx.query.get("jql").map(|v| v.as_str()).unwrap_or("");

    let db = ctx.db.clone().expect("DB connection missing");

    println!("====>>query_input {:?}", query_input);

    if query_input.is_empty() {
        return Response::ok(render_empty_matrix_interface().into_string());
    }

    // Attempt to evaluate user expression via string token rules engine
    let parsed_spec = match CustomQuerySpec::parse_from_str(query_input) {
        Ok(spec) => spec,
        Err(err) => {
            println!("====>>Parsed {:?}", err);
            return Response::ok(html! {
            div id="matrix-wrapper" class="bg-red-950/20 border border-red-900/50 rounded-xl p-4 font-mono text-xs text-red-400" {
                span class="font-bold uppercase block mb-1" { "⚠️ Syntax Mapping Error:" }
                (err)
            }
        }.into_string());
        }
    };

    println!("====>>Parsed {:?}", parsed_spec);

    // Automatically resolve whether the active runtime target is Postgres, MySQL, or SQLite
    let db_backend = db.get_database_backend();
    let native_stmt = JqlCompiler::compile(&parsed_spec, db_backend);

    println!("=====> {:?}", native_stmt);

    match db.query_all(native_stmt).await {
        Ok(query_results) => {
            if query_results.is_empty() {
                return Response::ok(html! {
                    div class="p-6 text-center text-gray-500 font-mono text-xs border border-gray-800 rounded-xl" {
                        "Query completed successfully. Execution returned an empty zero-row dataset."
                    }
                }.into_string());
            }
                println!("=====> {:?}", query_results);

            // Dynamically discover what columns came back inside the generic payload matrix
            let column_headers: Vec<String> = parsed_spec
                .select_columns
                .iter()
                .map(|(tbl, col)| {
                    tbl.as_ref()
                        .map_or(col.clone(), |t| format!("{}.{}", t, col))
                })
                .collect();

            let rendered_view = render_results_grid(&column_headers, &query_results);
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

fn render_results_grid(headers: &[String], rows: &[QueryResult]) -> Markup {
    html! {
        div id="matrix-wrapper" class="space-y-4" {
            div class="text-xxs font-mono uppercase tracking-wider text-emerald-500 font-bold" {
                "Result Matrix View"
            }
            div class="bg-gray-950 border border-gray-800 rounded-xl overflow-x-auto shadow-xl" {
                table class="w-full text-left border-collapse" {
                    thead class="bg-gray-900/80 border-b border-gray-800 text-xs font-semibold uppercase tracking-wider text-gray-400 font-mono" {
                        tr class="divide-x divide-gray-800" {
                            @for header in headers {
                                th class="p-3" { (header) }
                            }
                        }
                    }
                    tbody id="table-body" class="divide-y divide-gray-800 text-sm font-medium text-gray-300 font-mono text-xs" {
                        @for row in rows {
                            tr class="divide-x divide-gray-800 hover:bg-gray-900/40 transition" {
                                @for header in headers {
                                    // Extract fields out of returned queries by simple positional index configuration or labels
                                    @let field_val = row
                                        .try_get_by_index::<String>(headers.iter().position(|h| h == header).unwrap_or(0))
                                        .unwrap_or_else(|_| "NULL".to_string());
                                    td class="p-3" { (field_val) }
                                }
                            }
                        }
                    }
                    div class="bg-gray-900/40 px-4 py-2 border-t border-gray-850 flex justify-between items-center text-xxs font-mono text-gray-500" {
                        span { "Metrics: " (rows.len()) " entries collected successfully" }
                        a href="#" hx-get=(format!("/admin/{}", "table_slug")) hx-target="#matrix-wrapper" hx-swap="outerHTML" class="text-emerald-500 hover:underline" { "Reset Grid Matrix ↺" }
                    }
                }
            }
        }
    }
}

fn render_empty_matrix_interface() -> Markup {
    html! {
        div class="p-8 text-center text-gray-500 font-mono text-xs border border-gray-800 border-dashed rounded-xl" {
            "Input custom workspace tracking logic strings above to view underlying engine definitions..."
        }
    }
}

/// Render a single record's full details (all columns) in a modal or dedicated page.
pub async fn handle_detail<R>(ctx: RequestContext, repo: R, table_slug: &'static str) -> Response
where
    R: GritRepository + Send + Sync + 'static,
    <R as GritRepository>::Model: Sync + Send,
    <R as GritRepository>::Id: std::str::FromStr,
    <<R as GritRepository>::Id as std::str::FromStr>::Err: std::fmt::Display,
{
    let id_str = match ctx.params.get("id").map(|v| v.as_str()) {
        Some(id) => id,
        None => return error_response("Missing record ID"),
    };
    let id = match id_str.parse::<<R as GritRepository>::Id>() {
        Ok(id) => id,
        Err(e) => return error_response(format!("Invalid ID: {}", e)),
    };
    let record = match repo.find_by_id(id).await {
        Ok(Some(r)) => r,
        Ok(None) => return error_response("Record not found"),
        Err(e) => return error_response(format!("DB error: {}", e)),
    };

    let is_htmx = ctx.req.has_header("hx-request");
    let route_patch_str = format!("/admin/{}/update-cell", table_slug);
    let route_list_str = format!("/admin/{}", table_slug);

    let detail_html = html! {
        div class="space-y-6" {
            div class="flex justify-between items-center" {
                h1 class="text-2xl font-bold tracking-tight" {
                    "Record " (id_str) " – " (table_slug)
                }
                a href=(route_list_str)
                  hx-get=(route_list_str)
                  hx-target="#main-content"
                  hx-push-url="true"
                  class="text-emerald-500 hover:underline text-sm" { "← Back to list" }
            }

            div class="bg-gray-950 border border-gray-800 rounded-xl overflow-hidden shadow-xl" {
                table class="w-full text-left border-collapse" {
                    tbody class="divide-y divide-gray-800" {
                        @for col in repo.grid_columns().iter() {
                            @let field_value = repo.get_field_as_string(&record, &col.name);
                            tr class="hover:bg-gray-900/40 transition" {
                                th class="p-4 text-xs font-semibold uppercase tracking-wider text-gray-400 w-1/4" {
                                    (col.label)
                                }
                                td class="p-4 text-sm font-medium" {
                                    @if col.is_editable {
                                        input type="text"
                                            value=(field_value)
                                            name=(col.name)
                                            hx-patch=(route_patch_str)
                                            hx-trigger="change, keyup[key=='Enter']"
                                            hx-target="this"
                                            hx-swap="outerHTML"
                                            hx-vals=(format!("{{\"id\": \"{}\", \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", id_str, col.name, table_slug))
                                            class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
                                    } @else {
                                        span class="px-2 py-1 text-gray-400 font-mono text-xs" { (field_value) }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    // If called via HTMX, return just the content; otherwise wrap in admin shell.
    if is_htmx {
        Response::ok(detail_html.into_string())
    } else {
        let title = format!("{} – Record {}", table_slug, id_str);
        admin_shell(&title, detail_html, false)
    }
}

/// Delete multiple records at once.
pub async fn handle_bulk_delete<R>(
    ctx: RequestContext,
    repo: R,
    _table_slug: &'static str,
) -> Response
where
    R: GritRepository + Send + Sync + 'static,
    <R as GritRepository>::Model: Sync + Send,
    <R as GritRepository>::Id: std::str::FromStr,
    <<R as GritRepository>::Id as std::str::FromStr>::Err: std::fmt::Display,
{
    let ids_str = match ctx.form.fields.get("ids") {
        Some(v) => v.as_str(),
        None => return error_response("Missing 'ids' field"),
    };
    let ids_str = Sanitizer::url_decode(ids_str);
    let ids: Vec<<R as GritRepository>::Id> = match ids_str
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.parse())
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(ids) => ids,
        Err(e) => return error_response(format!("Invalid ID list: {}", e)),
    };
    if ids.is_empty() {
        return error_response("No IDs provided");
    }

    let db = repo.get_db();
    let txn = match db.begin().await {
        Ok(t) => t,
        Err(e) => return error_response(format!("Transaction start failed: {}", e)),
    };

    let mut errors = Vec::new();
    for id in ids {
        if let Err(e) = repo.delete_by_id(id).await {
            errors.push(format!("{}", e));
        }
    }

    if let Err(e) = txn.commit().await {
        return error_response(format!("Commit failed: {}", e));
    }

    if errors.is_empty() {
        Response::ok("")
    } else {
        error_response(format!("Some deletions failed: {}", errors.join("; ")))
    }
}

fn error_response(msg: impl ToString) -> Response {
    let msg = msg.to_string();
    let mut res = Response::new(400, Sanitizer::trust(&msg));
    // Set HX-Trigger header to show a toast
    let trigger = format!(
        r#"{{"showToast": {{"message": "{}", "type": "error"}}}}"#,
        msg.replace('"', "\\\"")
    );
    res.headers.push(("hx-trigger".to_string(), trigger));
    res
}

fn shield_error_response(err: ShieldError) -> Response {
    let msg = match err {
        ShieldError::BadRequest(s) => s,
        ShieldError::NotFound => "Resource not found".to_string(),
        ShieldError::UnauthorizedAccess => "Unauthorized".to_string(),
        ShieldError::Forbidden => "Forbidden".to_string(),
        _ => "Internal server error".to_string(),
    };
    error_response(msg)
}
