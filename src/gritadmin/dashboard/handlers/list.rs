use crate::admin_shell;
use crate::database::repository::jql::DynamicColumnSpec;
use crate::database::repository::registry::{ACTIONS_REGISTRY, ADMIN_REGISTRY};
use crate::database::repository::GritRepository;
use crate::gritadmin::dashboard::{
    build_page_window, error_response, render_grid_rows, success_response,
};
use crate::prelude::*;
use crate::security::xss::Sanitizer;
use maud::html;
use sea_orm::sea_query::{Alias, ColumnDef, Table};
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::QueryResult;
use sea_orm::{Condition, EntityTrait, PaginatorTrait, QueryOrder};
use std::collections::HashMap;

/// Generic dashboard view runner for listing data rows and handling infinite scrolls.
pub async fn handle_list<R>(ctx: RequestContext, repo: R, table_slug: &'static str) -> Response
where
    R: GritRepository + Send + Sync + 'static,
    <R as GritRepository>::Model: Sync + Send,
    <R as GritRepository>::Id: std::str::FromStr,
    <<R as GritRepository>::Id as std::str::FromStr>::Err: std::fmt::Display,
{
    use sea_orm::Condition;

    let is_htmx = ctx.req.has_header("hx-request");
    let db = repo.get_db();

    // ---- Parse Filters ----
    let mut op_map: HashMap<String, String> = HashMap::new();
    let mut val_map: HashMap<String, String> = HashMap::new();
    let mut search_q = None;
    let mut infinite_scroll = true; // On by default

    for (key, value) in ctx.query.iter() {
        // Decode special characters immediately at the entry boundary
        let decoded_value = value.first()
            .and_then(|v| urlencoding::decode(v.as_str()).map(|s| s.into_owned()).ok())
            .unwrap_or_else(|| value.first().map(|v| v.as_str().to_string()).unwrap_or_default());

        if let Some(stripped) = key.strip_prefix("filter__") {
            if let Some(rest) = stripped.strip_suffix("__op") {
                op_map.insert(rest.to_string(), decoded_value);
            } else if let Some(rest) = stripped.strip_suffix("__value") {
                val_map.insert(rest.to_string(), decoded_value);
            }
        } else if key == "q" {
            search_q = Some(decoded_value);
        } else if key == "infinite" {
            infinite_scroll = decoded_value == "true";
        }
    }

    // Combine ops and values
    // Only treat a column as "filtered" if the user actually picked an operator for it.
    // The filter form always submits every grid column's select/input (even ones nobody
    // touched), and those used to default to a real "contains" op with an empty value —
    // "contains('')" is a SQL LIKE '%%', which evaluates to NULL (excluded) on any row
    // where that untouched column happens to be NULL. So every extra untouched column
    // silently ANDed in a chance-to-exclude-rows condition, making combined filters look
    // like only one of them was "winning". Skipping unset columns entirely fixes that.
    let mut filters: HashMap<String, (String, String)> = HashMap::new();
    for col in op_map.keys() {
        if let Some(op) = op_map.get(col) {
            if op.is_empty() {
                continue; // "— no filter —" selected: this column isn't being filtered
            }
            let val = val_map.get(col).cloned().unwrap_or_default();
            if val.is_empty() && op != "is_null" && op != "is_not_null" {
                continue; // no value entered: don't apply an accidental empty-match filter
            }
            filters.insert(col.clone(), (op.clone(), val));
        }
    }

    // ---- Sorting ----
    let sort_col = ctx.query.get("sort").and_then(|v| v.first()).map(|v| v.as_str()).unwrap_or("");
    let sort_dir = ctx
        .query
        .get("direction")
        .and_then(|v| v.first())
        .map(|v| v.as_str())
        .unwrap_or("desc");
    let mut query = <R::Entity as EntityTrait>::find();

    // ---- Apply Filters ----
    let mut cond = Condition::all();

    for (col_name, (op, val)) in filters.iter() {
        if let Some(column) = repo.column_from_str(col_name) {
            match op.as_str() {
                "eq" => cond = cond.add(column.eq(val.clone())),
                "ne" => cond = cond.add(column.ne(val.clone())),
                "gt" => cond = cond.add(column.gt(val.clone())),
                "gte" => cond = cond.add(column.gte(val.clone())),
                "lt" => cond = cond.add(column.lt(val.clone())),
                "lte" => cond = cond.add(column.lte(val.clone())),
                "contains" => cond = cond.add(column.contains(val.clone())),
                "startswith" => cond = cond.add(column.starts_with(val.clone())),
                "endswith" => cond = cond.add(column.ends_with(val.clone())),
                "is_null" => cond = cond.add(column.is_null()),
                "is_not_null" => cond = cond.add(column.is_not_null()),
                _ => {}
            }
        }
    }

    // 2. Apply global search (q) – uses searchable columns
    if let Some(ref q) = search_q {
        if !q.is_empty() {
            let searchable: Vec<String> = repo
                .grid_columns()
                .iter()
                .map(|c| c.name.to_string())
                .collect();
            let mut search_cond = Condition::any();
            for col_name in searchable {
                if let Some(column) = repo.column_from_str(&col_name) {
                    search_cond = search_cond.add(column.contains(q.clone()));
                }
            }
            cond = cond.add(search_cond);
        }
    }

    query = query.filter(cond);

    // ---- Apply Sorting ----
    if let Some(col) = repo.column_from_str(sort_col) {
        if sort_dir == "asc" {
            query = query.order_by_asc(col);
        } else {
            query = query.order_by_desc(col);
        }
    } else {
        query = query.order_by_desc(R::id_column());
    }

    // ---- Pagination ----
    let page = ctx
        .query
        .get("page")
        .and_then(|v| v.first())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);

    // ---- Panel target detection ----
    // Every hx-get this handler renders links for tags itself with `partial=`, so we always
    // know exactly how much of the page to render back, instead of guessing from `page > 0`
    // (which broke as soon as a second page-driven feature — page-jump — was introduced):
    //   - partial=rows   → the infinite-scroll spinner asking for the next chunk of <tr>s
    //   - partial=matrix → sort/filter/JQL-reset/page-jump asking to replace #matrix-wrapper
    //   - (absent)       → a top-level route visit (sidebar, command palette, FK link, or a
    //                      plain browser load) that should render the FULL workspace view,
    //                      JQL explorer included, whether or not it's an htmx request.
    let partial = ctx.query.get("partial").and_then(|v| v.first()).map(|v| v.as_str()).unwrap_or("");
    let is_row_append = partial == "rows";
    let is_matrix_only = partial == "matrix";
    let page_size = 15;

    let paginator = query.paginate(db, page_size);
    let total_pages = paginator.num_pages().await.unwrap_or(0);
    let total_items = paginator.num_items().await.unwrap_or(0);
    let items = paginator.fetch_page(page).await.unwrap_or_default();

    // ---- Build URLs with filters preserved ----
    let route_path_str = format!("/admin/{}", table_slug);
    let _route_patch_str = format!("/admin/{}/update-cell", table_slug);
    let _route_delete_str = format!("/admin/{}/delete", table_slug);
    let route_advanced_str = format!("/admin/{}/query-explorer", table_slug);
    let _route_detail_str = format!("/admin/{}/", table_slug);
    let route_bulk_delete_str = format!("/admin/{}/bulk-delete", table_slug);

    // Helper to build query string with current filters, sort, and page
    let build_query_string =
        |page_override: Option<u64>, sort_override: Option<(String, String)>| {
            let mut parts = Vec::new();
            // Add filters: output both op and value parameters
            for (col, (op, val)) in filters.iter() {
                parts.push(format!("filter__{}__op={}", col, op));
                if op != "is_null" && op != "is_not_null" {
                    // parts.push(format!("filter__{}__value={}", col, val));
                    parts.push(format!(
                        "filter__{}__value={}",
                        col,
                        Sanitizer::url_encode(val)
                    ));
                }
            }
            // Add search q
            if let Some(q) = &search_q {
                parts.push(format!("q={}", Sanitizer::url_encode(q)));
            }
            // Retain scroll preference state across page jumps/sort actions
            parts.push(format!("infinite={}", infinite_scroll));

            // Sort
            let (s_col, s_dir) = match sort_override {
                Some((c, d)) => (c, d),
                None => (sort_col.to_string(), sort_dir.to_string()),
            };
            if !s_col.is_empty() {
                parts.push(format!("sort={}", s_col));
                parts.push(format!("direction={}", s_dir));
            }
            // Page
            if let Some(p) = page_override {
                parts.push(format!("page={}", p));
            } else if page > 0 {
                parts.push(format!("page={}", page));
            }
            if parts.is_empty() {
                String::new()
            } else {
                format!("?{}", parts.join("&"))
            }
        };

    // Tags a query string (as produced by build_query_string, which may be "") with a
    // `partial=` marker so handle_list knows how much of the page to send back.
    let with_partial = |qs: String, mode: &str| -> String {
        if qs.is_empty() {
            format!("?partial={}", mode)
        } else {
            format!("{}&partial={}", qs, mode)
        }
    };

    // Sort link helper — always a matrix-only refresh (replaces #matrix-wrapper)
    let sort_link = |col: &str| {
        let new_dir = if sort_col == col && sort_dir == "asc" {
            "desc"
        } else {
            "asc"
        };
        format!(
            "{}{}",
            route_path_str,
            with_partial(
                build_query_string(None, Some((col.to_string(), new_dir.to_string()))),
                "matrix"
            )
        )
    };

    // Page-jump link helper (pagination bar) — takes a 0-indexed target page and, like
    // sort_link, always asks for a matrix-only refresh so it fully replaces the current
    // page's rows instead of appending (that's the infinite-scroll spinner's job).
    let page_url = |target_page: u64| {
        format!(
            "{}{}",
            route_path_str,
            with_partial(build_query_string(Some(target_page), None), "matrix")
        )
    };

    // Base query string (filters + sort + partial=matrix, deliberately WITHOUT a page
    // number) for the numeric "jump to page" input — the input supplies its own page
    // number via hx-vals, so baking one in here would collide with it.
    let jump_base_qs = {
        let mut parts = Vec::new();
        for (col, (op, val)) in filters.iter() {
            parts.push(format!("filter__{}__op={}", col, op));
            if op != "is_null" && op != "is_not_null" {
                parts.push(format!(
                    "filter__{}__value={}",
                    col,
                    Sanitizer::url_encode(val)
                ));
            }
        }
        if let Some(q) = &search_q {
            parts.push(format!("q={}", Sanitizer::url_encode(q)));
        }
        if !sort_col.is_empty() {
            parts.push(format!("sort={}", sort_col));
            parts.push(format!("direction={}", sort_dir));
        }
        parts.push(format!("infinite={}", infinite_scroll));
        parts.push("partial=matrix".to_string());
        format!("?{}", parts.join("&"))
    };

    // ---- Render rows (shared with handle_search) ----
    let rows_html = html! {
        (render_grid_rows(&repo, &items, table_slug, true))
        @if infinite_scroll && (page + 1) < total_pages {
            tr id="infinite-scroll-spinner"
                hx-get=(format!("{}{}", route_path_str, with_partial(build_query_string(Some(page + 1), None), "rows")))
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

    // Build query string for export (only filters and q, no sort/page)
    let export_query_string = {
        let mut parts = Vec::new();
        for (col, (op, val)) in filters.iter() {
            parts.push(format!("filter__{}__op={}", col, op));
            if op != "is_null" && op != "is_not_null" {
                parts.push(format!(
                    "filter__{}__value={}",
                    col,
                    Sanitizer::url_encode(val)
                ));
            }
        }
        if let Some(q) = &search_q {
            parts.push(format!("q={}", Sanitizer::url_encode(q)));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("?{}", parts.join("&"))
        }
    };
    let route_export_str = format!("/admin/{}/export", table_slug);

    // ---- Build filter bar ----
    let filter_bar = html! {
        div class="bg-gray-950 border border-gray-800 rounded-xl p-4 mb-4 shadow-xl space-y-3" {
            form
                hx-get=(format!("{}?partial=matrix", route_path_str))
                hx-target="#matrix-wrapper"
                hx-swap="outerHTML"
                class="space-y-3" {
                    div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-3" {
                        @for col in repo.grid_columns().iter() {
                            @let current_op = filters.get(col.name).map(|(op, _)| op.as_str()).unwrap_or("");
                            @let current_val = filters.get(col.name).map(|(_, v)| v.as_str()).unwrap_or("");
                            div class="flex items-center gap-1" {
                                label class="text-xxs font-mono text-gray-500 w-16 truncate" { (col.label) }
                                select name=(format!("filter__{}__op", col.name)) class="bg-gray-900 border border-gray-800 rounded px-1 py-0.5 text-xs text-gray-300 focus:outline-none focus:border-emerald-500" {
                                    option value="" selected?[current_op.is_empty()] { "— no filter —" }
                                    option value="contains" selected?[current_op == "contains"] { "contains" }
                                    option value="eq" selected?[current_op == "eq"] { "=" }
                                    option value="ne" selected?[current_op == "ne"] { "≠" }
                                    option value="gt" selected?[current_op == "gt"] { ">" }
                                    option value="gte" selected?[current_op == "gte"] { "≥" }
                                    option value="lt" selected?[current_op == "lt"] { "<" }
                                    option value="lte" selected?[current_op == "lte"] { "≤" }
                                    option value="startswith" selected?[current_op == "startswith"] { "starts" }
                                    option value="endswith" selected?[current_op == "endswith"] { "ends" }
                                    option value="is_null" selected?[current_op == "is_null"] { "is null" }
                                    option value="is_not_null" selected?[current_op == "is_not_null"] { "not null" }
                                }
                                input type="text"
                                    name=(format!("filter__{}__value", col.name))
                                    value=(current_val)
                                    placeholder="value"
                                    class="bg-gray-900 border border-gray-800 rounded px-2 py-0.5 text-xs flex-1 min-w-0 focus:outline-none focus:border-emerald-500";
                            }
                        }
                    }
                    div class="flex items-center gap-3 pt-1" {
                        button type="submit"
                            class="bg-emerald-950/40 hover:bg-emerald-900/40 text-emerald-400 text-xs font-mono font-semibold px-4 py-1.5 rounded-lg transition duration-150" {
                            "Apply Filters"
                        }

                        a href=(format!("{}{}", route_export_str, export_query_string))
                        download
                        class="text-blue-400 hover:text-blue-300 text-xs font-mono underline" {
                            "⬇️ Export CSV"
                        }
                        a href=(route_path_str)
                          hx-get=(format!("{}?partial=matrix", route_path_str))
                          hx-target="#matrix-wrapper"
                          hx-swap="outerHTML"
                          class="text-gray-400 hover:text-gray-300 text-xs font-mono underline" {
                              "Clear"
                        }
                        // --- Scroll Toggle Control ---
                        span class="text-gray-800 font-mono text-xs mx-1" { "|" }
                        label class="text-xxs font-mono text-gray-500" { "Navigation:" }
                        select name="infinite"
                            hx-get=(format!("{}?partial=matrix", route_path_str))
                            hx-include="closest form"
                            hx-target="#matrix-wrapper"
                            hx-swap="outerHTML"
                            class="bg-gray-900 border border-gray-800 rounded px-2 py-0.5 text-xs text-gray-300 focus:outline-none focus:border-emerald-500 cursor-pointer" {

                            // Use ?[condition] to completely omit the attribute when false
                            option value="true" selected?[infinite_scroll] { "Infinite Scroll" }
                            option value="false" selected?[!infinite_scroll] { "Standard Pagination" }
                        }
                        @if !filters.is_empty() || search_q.is_some() {
                            span class="text-xxs text-emerald-500 font-mono" {
                                (&(filters.len() + if search_q.is_some() { 1 } else { 0 })) " active filter(s)"
                            }
                        }
                    }
            }
        }
    };

    let add_column_html = html! {
        // --- LUXURY SCHEMA EVOLUTION MODAL OVERLAY ---
        div id="evolve-schema-modal" class="hidden fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-sm p-4 animate-fade-in" {
            div class="bg-gray-950 border border-gray-800 rounded-2xl max-w-md w-full flex flex-col shadow-2xl overflow-hidden" {

                // Modal Header
                div class="p-5 border-b border-gray-800 flex justify-between items-center bg-gray-900/40" {
                    div {
                        h3 class="text-sm font-bold font-mono text-blue-400" { "Schema Evolution Engine" }
                        p class="text-xxs font-mono text-gray-400 mt-0.5" {
                            (format!("Append columns dynamically to table: '{}'", table_slug))
                        }
                    }
                    button
                        type="button"
                        onclick="document.getElementById('evolve-schema-modal').classList.add('hidden')"
                        class="text-gray-500 hover:text-white transition text-sm font-mono p-1" { "✕" }
                }

                // Modal Body (Form Container)
                form hx-post=(format!("/admin/api/alter-table/{}/add-column", table_slug))
                    hx-target="this"
                    hx-swap="none"
                    class="p-6 space-y-5" {

                    // New Column Identifier input block
                    div class="space-y-1.5" {
                        label class="block text-xxs font-mono font-semibold uppercase tracking-wider text-gray-400" { "Column Identifier" }
                        input type="text"
                            name="column_name"
                            required
                            placeholder="e.g., secondary_email"
                            class="bg-gray-900 border border-gray-800 rounded-lg px-4 py-2.5 w-full text-xs font-mono text-blue-400 focus:outline-none focus:border-blue-500 placeholder-gray-700 shadow-inner";
                    }

                    // Data Type dropdown constraint block
                    div class="space-y-1.5" {
                        label class="block text-xxs font-mono font-semibold uppercase tracking-wider text-gray-400" { "Field Native Type Map" }
                        select name="column_type"
                            class="bg-gray-900 border border-gray-800 rounded-lg px-4 py-2.5 w-full text-xs font-mono text-gray-300 focus:outline-none focus:border-blue-500 shadow-inner" {
                                option value="string" { "String / Text" }
                                option value="int" { "Integer" }
                                option value="bool" { "Boolean" }
                                option value="datetime" { "DateTime" }
                                option value="float" { "Float / Real" }
                        }
                        p class="text-[4px] font-mono text-gray-500 mt-1 leading-normal" {
                            "Live table appends are injected as Nullable."
                        }
                    }

                    // Footer Actions Container
                    div class="pt-4 border-t border-gray-800 flex justify-end space-x-3 bg-gray-950" {
                        button
                            type="button"
                            onclick="document.getElementById('evolve-schema-modal').classList.add('hidden')"
                            class="bg-gray-900 hover:bg-gray-800 border border-gray-800 text-gray-400 text-xs font-mono px-4 py-2 rounded-lg transition" {
                                "Cancel"
                        }
                        button
                            type="submit"
                            class="bg-blue-900/50 border border-blue-700/60 hover:bg-blue-800/50 text-blue-300 text-xs font-mono font-bold px-5 py-2 rounded-lg transition shadow-md" {
                                "＋ Append Column"
                        }
                    }
                }
            }
        }
        button
            onclick="document.getElementById('evolve-schema-modal').classList.remove('hidden')"
            class="bg-emerald-950/40 w-1/2 border border-emerald-800/60 hover:bg-emerald-900/40 text-emerald-400 text-xxs font-mono font-semibold px-3 py-1.5 rounded-lg transition duration-150 shadow-md" {
            "+ Add Column"
        }
        button
            hx-get=(format!("/admin/{}/bulk-create-modal", table_slug))
            hx-target="#modals-container"
            hx-swap="innerHTML"
            hx-indicator="body"
            class="bg-emerald-950/40 w-1/2 border border-emerald-800/60 hover:bg-emerald-900/40 text-emerald-400 text-xxs font-mono font-semibold px-3 py-1.5 rounded-lg transition duration-150 shadow-md" {
                "📦 Bulk Import"
        }
    };

    // ---- Build matrix wrapper (includes filter bar + table) ----
    let matrix_html = html! {
        div id="matrix-wrapper" class="space-y-4" {
            (filter_bar)
            (add_column_html)
            div class="bg-gray-950 border border-gray-800 rounded-xl shadow-xl overflow-x-auto table-scroll" {
                table class="w-full table-auto text-left border-collapse" {
                    thead class="bg-gray-900/80 border-b border-gray-800 text-xs font-semibold uppercase tracking-wider text-gray-400" {
                        tr class="divide-x divide-gray-800" {
                            th class="p-4 text-center w-10" { "" }
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
                    // Bulk Custom Actions Dropdown
                    @if let Some(actions) = ACTIONS_REGISTRY.lock().unwrap().get(table_slug) {
                        @if !actions.is_empty() {
                            div class="relative inline-block" {
                                button
                                    class="bg-blue-950/40 hover:bg-blue-900/40 text-blue-400 text-xs font-mono font-semibold px-4 py-2 rounded-lg transition duration-150 disabled:opacity-50 disabled:cursor-not-allowed"
                                    id="bulk-action-btn"
                                    disabled
                                    onclick="this.nextElementSibling.classList.toggle('hidden')" {
                                        "⚡ Bulk Actions"
                                    }
                                div class="hidden absolute left-0 mt-1 w-48 bg-gray-950 border border-gray-800 rounded-lg shadow-xl z-10" {
                                    @for action in actions {
                                        button
                                            hx-post=(format!("/admin/{}/bulk-action/{}", table_slug, action.label))
                                            hx-vals="ids=[]"
                                            hx-include="[name='selected_ids']"
                                            hx-target="this"
                                            hx-swap="none"
                                            hx-confirm=(format!("Execute '{}' on all selected records?", action.label))
                                            class=(format!("block w-full text-left px-3 py-2 text-xs font-mono hover:bg-gray-800/60 transition {}", action.color)) {
                                                @if let Some(icon) = action.icon {
                                                    (icon) " "
                                                }
                                                (action.label)
                                            }
                                    }
                                }
                            }
                        }
                    }
                }
                @if total_pages > 1 {
                    div class="bg-gray-950 border-t border-gray-800 px-4 py-3 flex flex-wrap items-center justify-between gap-3" {
                        span class="text-xxs font-mono text-gray-500" {
                            "Page " span class="text-gray-300 font-semibold" { (&(page + 1)) } " of " (total_pages)
                            " · " (total_items) " record" @if total_items != 1 { "s" }
                        }
                        div class="flex items-center gap-1" {
                            @if page > 0 {
                                a href=(page_url(page - 1))
                                  hx-get=(page_url(page - 1))
                                  hx-target="#matrix-wrapper"
                                  hx-swap="outerHTML"
                                  hx-push-url="true"
                                  class="px-2 py-1 text-xs font-mono rounded bg-gray-800 hover:bg-gray-700 text-gray-300 transition" { "‹ Prev" }
                            } @else {
                                span class="px-2 py-1 text-xs font-mono rounded bg-gray-900 text-gray-700 cursor-not-allowed" { "‹ Prev" }
                            }

                            @for entry in build_page_window(page + 1, total_pages) {
                                @match entry {
                                    Some(p) => {
                                        @if p == page + 1 {
                                            span class="px-2.5 py-1 text-xs font-mono rounded bg-emerald-950/50 text-emerald-400 border border-emerald-800/60" { (p) }
                                        } @else {
                                            a href=(page_url(p - 1))
                                              hx-get=(page_url(p - 1))
                                              hx-target="#matrix-wrapper"
                                              hx-swap="outerHTML"
                                              hx-push-url="true"
                                              class="px-2.5 py-1 text-xs font-mono rounded bg-gray-800 hover:bg-gray-700 text-gray-300 transition" { (p) }
                                        }
                                    }
                                    None => {
                                        span class="px-1 text-xs text-gray-600 select-none" { "…" }
                                    }
                                }
                            }

                            @if (page + 1) < total_pages {
                                a href=(page_url(page + 1))
                                  hx-get=(page_url(page + 1))
                                  hx-target="#matrix-wrapper"
                                  hx-swap="outerHTML"
                                  hx-push-url="true"
                                  class="px-2 py-1 text-xs font-mono rounded bg-gray-800 hover:bg-gray-700 text-gray-300 transition" { "Next ›" }
                            } @else {
                                span class="px-2 py-1 text-xs font-mono rounded bg-gray-900 text-gray-700 cursor-not-allowed" { "Next ›" }
                            }

                            span class="w-px h-5 bg-gray-800 mx-1" {}

                            label class="text-xxs font-mono text-gray-500" { "Go to" }
                            input type="number"
                                name="page_display"
                                min="1"
                                max=(total_pages)
                                value=(&(page + 1))
                                hx-get=(format!("{}{}", route_path_str, jump_base_qs))
                                hx-trigger="keyup[key=='Enter'] changed delay:500ms"
                                hx-target="#matrix-wrapper"
                                hx-swap="outerHTML"
                                hx-push-url="true"
                                hx-vals=(format!("js:{{page: Math.max(0, Math.min((this.value|0) - 1, {}))}}", total_pages.saturating_sub(1)))
                                class="w-16 bg-gray-900 border border-gray-800 rounded px-2 py-1 text-xs text-center focus:outline-none focus:border-emerald-500";
                        }
                    }
                }
            }
        }
    };

    // ---- Return response ----
    if is_row_append {
        // Infinite-scroll spinner: append-only, just the new <tr>s.
        Response::ok(rows_html.into_string())
    } else if is_matrix_only {
        // Sort / filter / clear / page-jump: replace #matrix-wrapper only, leave the
        // JQL explorer and title above it untouched.
        Response::ok(matrix_html.into_string())
    } else {
        // Top-level route visit — sidebar nav, command palette, a FK "jump to related
        // table" link, or a plain (non-htmx) browser load. All of these swap in the
        // FULL workspace view, so the JQL explorer stays visible no matter how you
        // arrived at this table. admin_shell itself still decides whether to wrap this
        // in the full <html> shell (non-htmx) or return it bare (htmx into #main-content).
        let display_title = format!("{} Workspace Matrix", table_slug.to_uppercase());
        let route_search_str = format!("/admin/{}/search", table_slug);

        let complete_view = html! {
            div class="space-y-6" {
            // JQL Explorer
            div id="jql-container" class="bg-gray-950 border border-gray-800 rounded-xl p-4 shadow-xl space-y-3 opacity-changing" {
                div class="flex items-center justify-between" {
                    h2 class="text-xs font-bold tracking-wider text-emerald-500 uppercase font-mono" {
                        "Matrix Query Explorer JQL"
                    }
                    span class="text-xxs font-mono text-gray-500" { "Supports SELECT ... FROM ... JOIN ... WHERE ..." }
                }
                div class="flex gap-2" {
                    input type="text"
                        id="jql-input"
                        name="jql"
                        placeholder="select id,title from projects join assignments on projects.id = assignments.project_id where status = 'active'"
                        hx-get=(route_advanced_str)
                        hx-trigger="keyup[key=='Enter']"
                        hx-target="#matrix-wrapper"
                        hx-indicator="#jql-container"
                        hx-swap="outerHTML"
                        class="bg-gray-900 border border-gray-800 rounded-lg px-4 py-2.5 flex-1 text-xs font-mono text-emerald-400 focus:outline-none focus:border-emerald-500 placeholder-gray-600 transition shadow-inner";

                    button
                        hx-get=(route_advanced_str)
                        hx-include="#jql-input"
                        hx-target="#matrix-wrapper"
                        hx-indicator="#jql-container"
                        hx-swap="outerHTML"
                        class="bg-emerald-950/40 border border-emerald-800/60 hover:bg-emerald-900/40 text-emerald-400 text-xs font-mono font-semibold px-4 py-2 rounded-lg transition duration-150 flex items-center justify-center space-x-2" {

                            // Using a unique display indicator class rule protects layout boundaries from HTMX's 'block' display override
                            span class="htmx-indicator animate-spin-custom leading-none flex items-center justify-center" {
                                svg class="w-3 h-3 text-emerald-400" fill="none" viewBox="0 0 24 24" {
                                    circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" {}
                                    path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" {}
                                }
                            }

                            span { "Run Query" }
                    }
                }
            }

                // Title and simple search
                div class="flex justify-between items-center" {
                    h1 class="text-2xl font-bold tracking-tight" { (display_title) }
                    input type="text"
                        name="q"
                        placeholder="Basic keyword lookup..."
                        hx-get=(route_search_str)
                        hx-trigger="keyup changed delay:300ms, search"
                        hx-target="#table-body"
                        hx-indicator="#search-spinner"
                        class="bg-gray-950 border border-gray-800 rounded-lg px-4 py-2 w-80 text-sm focus:outline-none focus:border-emerald-500 transition";
                    // The inline absolute loader matching our targeted ID
                    div id="search-spinner" class="htmx-indicator absolute left-2.5 top-1/2 -translate-y-1/2" {
                        svg class="animate-spin-custom h-3.5 w-3.5 text-emerald-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" {
                            circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" {}
                            path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" {}
                        }
                    }
                }

                (matrix_html)
            }
        };
        admin_shell(&display_title, complete_view, is_htmx)
    }
}
