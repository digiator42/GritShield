use std::collections::HashMap;

use crate::database::repository::{AdminHandlerFn, GridColumn};
use crate::database::repository::{CustomQuerySpec, JoinSpec, JqlCompiler, WhereSpec};
use crate::database::repository::{GritRepository, ADMIN_REGISTRY};
use crate::deps::sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityName, EntityTrait,
    PaginatorTrait, QueryOrder, Statement, TransactionTrait,
};
use crate::gritadmin::models::audit_log;
use crate::security::errors::ShieldError;
use crate::security::xss::UntrustedString;
use crate::{admin_shell, prelude::*};
use maud::html;
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::QueryResult;

/// Check if a column name looks like a foreign key.
fn is_foreign_key_column(col_name: &str) -> bool {
    col_name.ends_with("_id")
}

/// Try to resolve the target table slug for a foreign key column.
/// e.g., "user_id" → "user" (or "users" if registered).
fn get_target_table_slug(col_name: &str) -> Option<String> {
    if !is_foreign_key_column(col_name) {
        return None;
    }

    // Remove "_id" suffix
    let base = col_name.trim_end_matches("_id");

    // Try both singular and plural forms
    let candidates = vec![base.to_string(), format!("{}s", base)];

    println!("======>> {:?}", candidates);

    let registry = ADMIN_REGISTRY.lock().unwrap();
    for candidate in candidates {
        if registry.contains_key(&candidate.as_str()) {
            return Some(candidate);
        }
    }
    None
}

/// Unified row renderer shared by the main matrix grid, quick search, and (via consistent
/// styling) the JQL result viewer. Centralizing this means inline-edit inputs, FK links,
/// and row actions never drift out of sync between the different ways rows get fetched.
fn render_grid_rows<R>(
    repo: &R,
    items: &[R::Model],
    table_slug: &'static str,
    show_checkbox: bool,
) -> Markup
where
    R: GritRepository,
{
    let route_patch_str = format!("/admin/{}/update-cell", table_slug);
    let route_delete_str = format!("/admin/{}/delete", table_slug);
    let route_detail_str = format!("/admin/{}/", table_slug);

    html! {
        @for item in items.iter() {
            @let record_id = repo.get_field_as_string(item, "id");
            tr class="divide-x divide-gray-800 hover:bg-gray-900/40 transition group" {
                @if show_checkbox {
                    td class="p-3 text-center w-10" {
                        input type="checkbox"
                            name="selected_ids"
                            value=(record_id)
                            class="form-checkbox bg-gray-800 border-gray-700 rounded text-emerald-500 focus:ring-0 focus:ring-offset-0";
                    }
                }
                @for col in repo.grid_columns().iter() {
                    @let raw_val = repo.get_field_as_string(item, &col.name);
                    td class="p-3 text-sm font-medium" {
                        @if is_foreign_key_column(&col.name) && !raw_val.is_empty() {
                            // Foreign keys always render as links (even if editable)
                            @if let Some(target_slug) = get_target_table_slug(&col.name) {
                                a href=(format!("/admin/{}/{}", target_slug, raw_val))
                                hx-get=(format!("/admin/{}/{}", target_slug, raw_val))
                                hx-target="#main-content"
                                hx-push-url="true"
                                class="text-blue-400 hover:text-blue-300 underline font-mono text-xs" {
                                    (raw_val)
                                }
                            } @else {
                                span class="px-2 py-1 text-gray-400 font-mono text-xs" { (raw_val) }
                            }
                        } @else if col.is_editable {
                            // Regular editable field (not a foreign key)
                            input type="text"
                                value=(raw_val)
                                name=(col.name)
                                hx-patch=(route_patch_str)
                                hx-trigger="change, keyup[key=='Enter']"
                                hx-target="this"
                                hx-swap="outerHTML"
                                hx-vals=(format!("{{\"id\": \"{}\", \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", record_id, col.name, table_slug))
                                class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
                        } @else {
                            // Read-only field (not a foreign key)
                            span class="px-2 py-1 text-gray-400 font-mono text-xs" { (raw_val) }
                        }
                    }
                }
                td class="p-3 text-center w-24" {
                    a href=(format!("{}{}", route_detail_str, record_id))
                      hx-get=(format!("{}{}", route_detail_str, record_id))
                      hx-target="#main-content"
                      hx-push-url="true"
                      class="text-blue-400/60 hover:text-blue-400 p-1 rounded hover:bg-blue-950/30 font-mono text-xs opacity-0 group-hover:opacity-100 transition duration-150 mr-2" {
                        "👁"
                    }
                    button
                        hx-delete=(route_delete_str)
                        hx-vals=(format!("{{\"id\": \"{}\"}}", record_id))
                        hx-target="closest tr"
                        hx-swap="outerHTML"
                        hx-confirm="Are you sure you want to permanently delete this record?"
                        class="text-red-500/60 hover:text-red-400 p-1 rounded hover:bg-red-950/30 font-mono text-xs opacity-0 group-hover:opacity-100 transition duration-150" {
                            "✕"
                    }
                }
            }
        }
    }
}

/// Build a windowed pagination sequence of 1-indexed page numbers with `None` standing
/// in for an ellipsis gap, e.g. current=6, total=20 → [1, …, 4, 5, 6, 7, 8, …, 20].
fn build_page_window(current_1based: u64, total: u64) -> Vec<Option<u64>> {
    if total <= 1 {
        return (1..=total).map(Some).collect();
    }

    let lo = current_1based.saturating_sub(2).max(1);
    let hi = (current_1based + 2).min(total);

    let mut pages: Vec<u64> = vec![1];
    for p in lo..=hi {
        if p != 1 {
            pages.push(p);
        }
    }
    if *pages.last().unwrap() != total {
        pages.push(total);
    }
    pages.dedup();

    let mut windowed = Vec::with_capacity(pages.len() + 2);
    for (i, &p) in pages.iter().enumerate() {
        if i > 0 && p > pages[i - 1] + 1 {
            windowed.push(None);
        }
        windowed.push(Some(p));
    }
    windowed
}

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

    for (key, value) in ctx.query.iter() {
        // Decode special characters immediately at the entry boundary
        let decoded_value = urlencoding::decode(value.as_str())
            .map(|s| s.into_owned())
            .unwrap_or_else(|_| value.as_str().to_string());

        if let Some(stripped) = key.strip_prefix("filter__") {
            if let Some(rest) = stripped.strip_suffix("__op") {
                op_map.insert(rest.to_string(), decoded_value);
            } else if let Some(rest) = stripped.strip_suffix("__value") {
                val_map.insert(rest.to_string(), decoded_value);
            }
        } else if key == "q" {
            search_q = Some(decoded_value);
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

    // ---- Debug output ----
    println!("===== Filters parsed: {:?}", filters);
    println!("===== Search q: {:?}", search_q);

    // ---- Sorting ----
    let sort_col = ctx.query.get("sort").map(|v| v.as_str()).unwrap_or("");
    let sort_dir = ctx
        .query
        .get("direction")
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
    let partial = ctx.query.get("partial").map(|v| v.as_str()).unwrap_or("");
    let is_row_append = partial == "rows";
    let is_matrix_only = partial == "matrix";
    let page_size = 15;

    let paginator = query.paginate(db, page_size);
    let total_pages = paginator.num_pages().await.unwrap_or(0);
    let total_items = paginator.num_items().await.unwrap_or(0);
    let items = paginator.fetch_page(page).await.unwrap_or_default();

    // ---- Build URLs with filters preserved ----
    let route_path_str = format!("/admin/{}", table_slug);
    let route_patch_str = format!("/admin/{}/update-cell", table_slug);
    let route_delete_str = format!("/admin/{}/delete", table_slug);
    let route_advanced_str = format!("/admin/{}/query-explorer", table_slug);
    let route_detail_str = format!("/admin/{}/", table_slug);
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
                    parts.push(format!("filter__{}__value={}", col, Sanitizer::url_encode(val)));
                }
            }
            // Add search q
            if let Some(q) = &search_q {
                parts.push(format!("q={}", Sanitizer::url_encode(q)));
            }
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
                parts.push(format!("filter__{}__value={}", col, Sanitizer::url_encode(val)));
            }
        }
        if let Some(q) = &search_q {
            parts.push(format!("q={}", Sanitizer::url_encode(q)));
        }
        if !sort_col.is_empty() {
            parts.push(format!("sort={}", sort_col));
            parts.push(format!("direction={}", sort_dir));
        }
        parts.push("partial=matrix".to_string());
        format!("?{}", parts.join("&"))
    };

    // ---- Render rows (shared with handle_search) ----
    let rows_html = html! {
        (render_grid_rows(&repo, &items, table_slug, true))
        @if (page + 1) < total_pages {
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
                parts.push(format!("filter__{}__value={}", col, Sanitizer::url_encode(val)));
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
                        @if !filters.is_empty() || search_q.is_some() {
                            span class="text-xxs text-emerald-500 font-mono" {
                                (&(filters.len() + if search_q.is_some() { 1 } else { 0 })) " active filter(s)"
                            }
                        }
                    }
            }
        }
    };

    // ---- Build matrix wrapper (includes filter bar + table) ----
    let matrix_html = html! {
        div id="matrix-wrapper" class="space-y-4" {
            (filter_bar)
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
                // JQL Explorer (unchanged)
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

                // Title and simple search (kept for compatibility)
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

                (matrix_html)
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

    // Same row template as the main matrix (checkbox, FK links, inline edit, view/delete
    // icons) — previously this duplicated an older, slightly different template that was
    // missing the checkbox column and FK linking, so bulk-select and "jump to related
    // record" silently didn't work when rows came from quick search.
    let rows_html = render_grid_rows(&repo, &items, table_slug, true);
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

    let user_id = ctx.get_user_id();

    match repo.delete_by_id(record_id, user_id.as_deref()).await {
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
    let form = ctx.form.clone().fields;
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

    let user_id = ctx.get_user_id();

    match repo
        .update_column_value(record_id, &column_name, target_value, user_id.as_deref())
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
                            table class="w-full text-left border-collapse pointer-events-none select-none opacity-85 table-scroll" {
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
                table class="w-full text-left border-collapse table-scroll" {
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

    println!(
        "===== Querying audit logs: table_slug={}, id_str={}",
        table_slug, id_str
    );

    // Fetch audit logs for this record
    let logs = audit_log::Entity::find()
        .filter(audit_log::Column::TableName.eq(repo.table_name()))
        .filter(audit_log::Column::RecordId.eq(id_str))
        .order_by_desc(audit_log::Column::Timestamp)
        .all(repo.get_db())
        .await
        .unwrap_or_default();

    println!("===== Found {} audit logs", logs.len());

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

            div class="bg-gray-950 border border-gray-800 rounded-xl shadow-xl overflow-x-auto" {
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
            div class="mt-8" {
                h2 class="text-lg font-semibold tracking-tight text-gray-300 mb-4" { "📜 Audit History" }
                div class="bg-gray-950 border border-gray-800 rounded-xl shadow-xl overflow-x-auto" {
                    table class="w-full text-left border-collapse" {
                        thead class="bg-gray-900/80 border-b border-gray-800 text-xs font-semibold uppercase tracking-wider text-gray-400" {
                            tr class="divide-x divide-gray-800" {
                                th class="p-3" { "Action" }
                                th class="p-3" { "User" }
                                th class="p-3" { "Timestamp" }
                                th class="p-3" { "Changes" }
                            }
                        }
                        tbody class="divide-y divide-gray-800" {
                            @for log in &logs {
                                tr class="hover:bg-gray-900/40 transition" {
                                    td class="p-3 text-xs font-mono" {
                                        span class={
                                            "px-2 py-1 rounded font-semibold "
                                            @if log.action == "delete" {
                                                "bg-red-950/30 text-red-400"
                                            } @else {
                                                "bg-emerald-950/30 text-emerald-400"
                                            }
                                        } {
                                            (log.action.to_uppercase())
                                        }
                                    }
                                    td class="p-3 text-xs font-mono text-gray-400" {
                                        @if let Some(ref uid) = log.user_id {
                                            "👤 " (uid)
                                        } @else {
                                            span class="text-gray-600" { "🤖 system" }
                                        }
                                    }
                                    td class="p-3 text-xs font-mono text-gray-400" {
                                        (log.timestamp.format("%Y-%m-%d %H:%M:%S"))
                                    }
                                    td class="p-3 text-xs font-mono" {
                                        @if let Some(old) = &log.old_values {
                                            @if let Some(new) = &log.new_values {
                                                div class="space-y-1" {
                                                    div class="text-red-400/70" {
                                                        "− " (old.to_string())
                                                    }
                                                    div class="text-emerald-400/70" {
                                                        "+ " (new.to_string())
                                                    }
                                                }
                                            } @else {
                                                div class="text-red-400" {
                                                    "🗑️ Deleted"
                                                }
                                            }
                                        } @else {
                                            span class="text-gray-500" { "—" }
                                        }
                                    }
                                }
                            }
                            @if logs.is_empty() {
                                tr {
                                    td colspan="4" class="p-8 text-center text-gray-500 text-xs" {
                                        "No changes recorded yet."
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

    let user_id = ctx.get_user_id();

    let mut errors = Vec::new();
    for id in ids {
        if let Err(e) = repo.delete_by_id(id, user_id.as_deref()).await {
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

/// Export current filtered dataset as CSV.
pub async fn handle_export<R>(ctx: RequestContext, repo: R, table_slug: &'static str) -> Response
where
    R: GritRepository + Send + Sync + 'static,
    <R as GritRepository>::Model: Sync + Send,
    <R as GritRepository>::Id: std::str::FromStr,
    <<R as GritRepository>::Id as std::str::FromStr>::Err: std::fmt::Display,
{
    use sea_orm::Condition;

    let db = repo.get_db();

    // ---- Parse Filters (same as handle_list) ----
    let mut op_map: HashMap<String, String> = HashMap::new();
    let mut val_map: HashMap<String, String> = HashMap::new();
    let mut search_q = None;

    for (key, value) in ctx.query.iter() {
        if let Some(stripped) = key.strip_prefix("filter__") {
            if let Some(rest) = stripped.strip_suffix("__op") {
                op_map.insert(rest.to_string(), value.as_str().to_string());
            } else if let Some(rest) = stripped.strip_suffix("__value") {
                val_map.insert(rest.to_string(), value.as_str().to_string());
            }
        } else if key == "q" {
            search_q = Some(value.as_str().to_string());
        }
    }

    let mut filters: HashMap<String, (String, String)> = HashMap::new();
    for col in op_map.keys() {
        if let Some(op) = op_map.get(col) {
            let val = val_map.get(col).cloned().unwrap_or_default();
            filters.insert(col.clone(), (op.clone(), val));
        }
    }

    // ---- Build Query with Filters (no sorting / no pagination) ----
    let mut query = <R::Entity as EntityTrait>::find();
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

    // ---- Fetch All Records (no limit) ----
    let records = match query.all(db).await {
        Ok(r) => r,
        Err(e) => return Response::bad_request(format!("DB error: {}", e)),
    };

    // ---- Generate CSV ----
    let columns = repo.grid_columns();
    let csv_content = export_to_csv(&records, &columns, &repo);

    // ---- Build Download Response ----
    let filename = format!(
        "{}_{}.csv",
        table_slug,
        chrono::Local::now().format("%Y%m%d_%H%M%S")
    );
    let mut res = Response::new(200, Sanitizer::trust(&csv_content));
    res.headers.push((
        "Content-Type".to_string(),
        "text/csv; charset=utf-8".to_string(),
    ));
    res.headers.push((
        "Content-Disposition".to_string(),
        format!("attachment; filename=\"{}\"", filename),
    ));
    res
}

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
    for (table_name, table_slug, _) in &table_infos {
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
        admin_shell("Dashboard", dashboard_html, false)
    }
}

/// Simple CSV writer (escapes commas and quotes).
fn export_to_csv<R>(records: &[R::Model], columns: &[GridColumn], repo: &R) -> String
where
    R: GritRepository,
{
    use std::fmt::Write;

    let mut csv = String::new();

    // Header
    let header: Vec<&str> = columns.iter().map(|c| c.label).collect();
    writeln!(csv, "{}", header.join(",")).ok();

    // Rows
    for record in records {
        let row: Vec<String> = columns
            .iter()
            .map(|col| {
                let val = repo.get_field_as_string(record, col.name);
                // Escape: wrap in quotes if it contains comma or quote
                if val.contains(',') || val.contains('"') {
                    format!("\"{}\"", val.replace('"', "\"\""))
                } else {
                    val
                }
            })
            .collect();
        writeln!(csv, "{}", row.join(",")).ok();
    }

    csv
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
