
use crate::database::repository::GritRepository;
use crate::database::repository::registry::{ACTIONS_REGISTRY, ADMIN_REGISTRY};
use crate::prelude::*;
use maud::Markup;
use sea_orm::QueryResult;
use super::{is_foreign_key_column, get_target_table_slug};
use crate::database::GridColumn;

/// Unified row renderer shared by the main matrix grid, quick search, and (via consistent
/// styling) the JQL result viewer. Centralizing this means inline-edit inputs, FK links,
/// and row actions never drift out of sync between the different ways rows get fetchedpub .
pub fn render_grid_rows<R>(
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
                            @if let Some(target_slug) = get_target_table_slug(&table_slug, &col.name) {
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

                    // ---- Custom Actions Dropdown ----
                    @if let Some(actions) = ACTIONS_REGISTRY.lock().unwrap().get(table_slug) {
                        div class="relative inline-block group-hover:opacity-100 opacity-0 transition duration-150" {
                            button
                                class="text-gray-400 hover:text-gray-300 font-mono text-xs p-1"
                                onclick="this.nextElementSibling.classList.toggle('hidden')" {
                                    "⚡"
                                }
                            div class="hidden absolute right-0 mt-1 w-36 bg-gray-950 border border-gray-800 rounded-lg shadow-xl z-10" {
                                @for action in actions {
                                    button
                                        hx-post=(format!("/admin/{}/action/{}", table_slug, action.label))
                                        hx-vals=(format!("{{\"ids\": [\"{}\"]}}", record_id))
                                        hx-target="this"
                                        hx-swap="none"
                                        hx-confirm=(format!("Execute '{}' on this record?", action.label))
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
        }
    }
}


pub fn render_results_grid<R>(
    headers: &[String],
    rows: &[QueryResult],
    table_slug: &str,
    repo: &R,
) -> Markup
where
    R: GritRepository,
{
    let route_patch_str = format!("/admin/{}/update-cell", table_slug);
    let route_delete_str = format!("/admin/{}/delete", table_slug);
    let route_detail_str = format!("/admin/{}/", table_slug);
    let grid_cols = repo.grid_columns();

    // Helper closure to safely extract dynamic record row IDs across driver naming variations
    let extract_record_id = |row: &QueryResult, header: &str, col_name: &str| -> Option<String> {
        if header.to_lowercase() == "id"
            || header.ends_with(".id")
            || col_name.to_lowercase() == "id"
        {
            if let Ok(Some(val)) = row
                .try_get::<Option<i64>>("", header)
                .or_else(|_| row.try_get::<Option<i64>>("", col_name))
            {
                return Some(val.to_string());
            }
            if let Ok(Some(val)) = row
                .try_get::<Option<i32>>("", header)
                .or_else(|_| row.try_get::<Option<i32>>("", col_name))
            {
                return Some(val.to_string());
            }
            if let Ok(Some(val)) = row
                .try_get::<Option<String>>("", header)
                .or_else(|_| row.try_get::<Option<String>>("", col_name))
            {
                return Some(val);
            }

            let h_lower = header.to_lowercase();
            let c_lower = col_name.to_lowercase();
            if let Ok(Some(val)) = row
                .try_get::<Option<i64>>("", &h_lower)
                .or_else(|_| row.try_get::<Option<i64>>("", &c_lower))
            {
                return Some(val.to_string());
            }
            if let Ok(Some(val)) = row
                .try_get::<Option<String>>("", &h_lower)
                .or_else(|_| row.try_get::<Option<String>>("", &c_lower))
            {
                return Some(val);
            }
        }
        None
    };

    // Helper closure to safely cascade lookup cell values across type primitives and name permutations
    let extract_cell_value = |row: &QueryResult, header: &str, col_name: &str| -> String {
        let lookups = [
            header,
            col_name,
            &header.to_lowercase(),
            &col_name.to_lowercase(),
        ];

        for key in lookups {
            if let Ok(Some(s)) = row.try_get::<Option<String>>("", key) {
                return s;
            }
            if let Ok(Some(i)) = row.try_get::<Option<i64>>("", key) {
                return i.to_string();
            }
            if let Ok(Some(i)) = row.try_get::<Option<i32>>("", key) {
                return i.to_string();
            }
            if let Ok(Some(b)) = row.try_get::<Option<bool>>("", key) {
                return b.to_string();
            }
            if let Ok(Some(f)) = row.try_get::<Option<f64>>("", key) {
                return f.to_string();
            }
        }
        "".to_string()
    };

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
                                th class="p-3 text-xs font-semibold text-gray-400" { (header) }
                            }
                            @if headers.iter().any(|h| {
                                let c = if let Some(idx) = h.rfind('.') { &h[idx + 1..] } else { h.as_str() };
                                c.to_lowercase() == "id"
                            }) {
                                th class="p-3 text-center w-24 text-emerald-400 font-bold font-mono text-xs" { "Actions" }
                            }
                        }
                    }
                    tbody id="table-body" class="divide-y divide-gray-850 text-xs font-medium text-gray-300 font-mono" {
                        @for row in rows {
                            // 1. Simplify ID tracking: extract substring and find the row ID value
                            @let record_id = {
                                let mut found_id = "null".to_string();
                                for h in headers {
                                    let c_name = if let Some(idx) = h.rfind('.') { &h[idx + 1..] } else { h.as_str() };
                                    if c_name.to_lowercase() == "id" {
                                        if let Some(id_str) = extract_record_id(row, h, c_name) {
                                            found_id = id_str;
                                            break;
                                        }
                                    }
                                }
                                found_id
                            };

                            tr class="divide-x divide-gray-800 hover:bg-gray-900/40 transition group" {
                                @for header in headers {
                                    @let col_name = if let Some(idx) = header.rfind('.') { &header[idx + 1..] } else { header.as_str() };
                                    @let cell_val = extract_cell_value(row, header, col_name);

                                    // 2. Simplify Editability: check if the short column name is registered as editable
                                    @let is_editable = grid_cols.iter().any(|c| {
                                        c.name.to_lowercase() == col_name.to_lowercase() && c.is_editable
                                    });
                                    td class="p-3 text-sm font-medium" {
                                        @if is_foreign_key_column(col_name) && !cell_val.is_empty() {
                                            @if let Some(target_slug) = get_target_table_slug(&table_slug, col_name) {
                                                a href=(format!("/admin/{}/{}", target_slug, cell_val))
                                                   hx-get=(format!("/admin/{}/{}", target_slug, cell_val))
                                                   hx-target="#main-content"
                                                   hx-push-url="true"
                                                   class="text-blue-400 hover:text-blue-300 underline font-mono text-xs" {
                                                    (cell_val)
                                                }
                                            } @else {
                                                span class="px-2 py-1 text-gray-400 font-mono text-xs" { (cell_val) }
                                            }
                                        } @else if is_editable && record_id != "null" {
                                            input type="text"
                                                value=(cell_val)
                                                name=(col_name)
                                                hx-patch=(route_patch_str)
                                                hx-trigger="change, keyup[key=='Enter']"
                                                hx-target="this"
                                                hx-swap="outerHTML"
                                                hx-vals=(format!("{{\"id\": \"{}\", \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", record_id, col_name, table_slug))
                                                class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
                                        } @else {
                                            span class="px-2 py-1 text-gray-400 font-mono text-xs" { (cell_val) }
                                        }
                                    }
                                }

                                // 3. Actions context visibility
                                @if headers.iter().any(|h| {
                                    let c = if let Some(idx) = h.rfind('.') { &h[idx + 1..] } else { h.as_str() };
                                    c.to_lowercase() == "id"
                                }) {
                                    td class="p-3 text-center w-24 whitespace-nowrap" {
                                        @if record_id != "null" {
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
                                        } @else {
                                            // The native title attribute bypasses all CSS overflow clipping bugs entirely
                                            div class="inline-block cursor-help select-none px-2 py-1 text-gray-600 font-mono text-xs"
                                                title="Record ID missing: Perhaps you query on different table" {
                                                "N/A"
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                div class="bg-gray-900/40 px-4 py-3 border-t border-gray-800 flex justify-between items-center text-xxs font-mono text-gray-500" {
                    span { "Metrics: " span class="text-emerald-400 font-semibold" { (rows.len()) } " entries collected successfully" }
                    a href=(format!("/admin/{}", table_slug))
                       hx-get=(format!("/admin/{}?partial=matrix", table_slug))
                       hx-target="#matrix-wrapper"
                       hx-swap="outerHTML"
                       class="text-emerald-500 hover:underline font-semibold font-mono" { "Reset Grid Matrix ↺" }
                }
            }
        }
    }
}


pub fn render_empty_matrix_interface() -> Markup {
    html! {
        div class="p-8 text-center text-gray-500 font-mono text-xs border border-gray-800 border-dashed rounded-xl" {
            "Input custom workspace tracking logic strings above to view underlying engine definitions..."
        }
    }
}
