use crate::{models::user, repository::user_repository::UserRepository, shell};
use gritshield::{
    database::repository::{GritRepository, ADMIN_REGISTRY},
    deps::serde_json,
    prelude::*,
};
use maud::PreEscaped;
use sea_orm::{
    ActiveModelTrait, EntityTrait, IntoActiveModel, PaginatorTrait, QueryOrder, QuerySelect,
};

pub struct AdminUserController;

#[controller("/admin")]
impl AdminUserController {
    #[get("/users")]
    pub async fn list_users(ctx: RequestContext) -> Response {
        let is_htmx = ctx.req.has_header("hx-request");
        let db = ctx.db.as_deref().unwrap();
        let user_repo = UserRepository { db: db.clone() };

        let columns = UserRepository::column_names();
        let raw_table_name = user_repo.table_name();

        let page = ctx
            .query
            .get("page")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);

        let is_infinite_scroll = is_htmx && page > 0;
        let page_size = 15;

        // Execute proper paginated database extraction boundaries
        let user_paginator = UserRepository::find()
            .order_by_desc(UserRepository::id_column())
            .paginate(db, page_size);

        let total_pages = user_paginator.num_pages().await.unwrap_or(0);
        let total_items = user_paginator.num_items().await.unwrap_or(0);
        let users = user_paginator.fetch_page(page).await.unwrap_or_default();

        // Generate standard rows + scroll rows, activating OOB updates ONLY on active scrolling
        let rendered_results = render_admin_table_view(
            &user_repo,
            &users,
            page,
            total_pages,
            total_items,
            columns.len(),
            is_infinite_scroll,
            None,
        );

        if is_infinite_scroll {
            Response::ok(rendered_results)
        } else {
            let display_title = format!(
                "{} Matrix Matrix",
                raw_table_name
                    .chars()
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_default()
                    + &raw_table_name[1..]
            );

            let complete_view = html! {
                div class="space-y-6" {
                    div class="flex justify-between items-center" {
                        h1 class="text-2xl font-bold tracking-tight" { (display_title) }
                        p class="text-xs text-gray-500 mt-1" {
                            (format!("Double-click field inputs to execute reactive backend inline-edits dynamically on table '{}'.", raw_table_name))
                        }

                        input type="text"
                            name="q"
                            placeholder="Type Alt+K to look up tables, or search records..."
                            hx-get=(format!("/admin/{}/search", raw_table_name))
                            hx-trigger="keyup changed delay:300ms"
                            hx-target="#table-body"
                            hx-indicator="#search-loading"
                            class="bg-gray-950 border border-gray-800 rounded px-4 py-2 w-80 text-sm focus:outline-none focus:border-emerald-500 transition";
                    }

                    div class="bg-gray-950 border border-gray-800 rounded-xl overflow-hidden shadow-xl relative" {
                        div id="search-loading"
                            class="htmx-indicator absolute inset-0 bg-gray-950/80 backdrop-blur-sm flex items-center justify-center z-10 rounded-xl" {
                            div class="flex flex-col items-center space-y-3" {
                                svg class="animate-spin h-8 w-8 text-emerald-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" {
                                    circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" {}
                                    path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" {}
                                }
                                span class="text-sm text-gray-400" { "Searching..." }
                            }
                        }

                        table class="w-full text-left border-collapse" {
                            thead class="bg-gray-900/80 backdrop-blur border-b border-gray-800 text-xs font-semibold uppercase tracking-wider text-gray-400" {
                                tr class="divide-x divide-gray-800" {
                                    @for col in user_repo.grid_columns().iter() {
                                        th class="p-4" { (col.label) }
                                    }
                                }
                            }
                            tbody id="table-body" class="divide-y divide-gray-800" {
                                (PreEscaped(rendered_results))
                            }
                            tfoot {
                                tr id="pagination-stats-target" class="bg-gray-950 border-t border-gray-800 text-xs font-medium" {
                                    td colspan=(columns.len()) class="p-4 bg-gray-950/90 backdrop-blur" {
                                        div class="flex justify-between items-center w-full" {
                                            span class="text-gray-400" {
                                                "Viewing Page Slice "
                                                span class="text-emerald-400 font-mono font-semibold" { (&(page + 1)) }
                                                " of "
                                                span class="text-gray-400 font-mono" { (total_pages) }
                                            }
                                            span class="text-xs text-gray-500 font-medium tracking-wide bg-gray-900 px-2.5 py-1 rounded-md border border-gray-800" {
                                                "Total Matched Rows: " span class="text-gray-300 font-mono" { (total_items) }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            };

            shell::admin_shell("Manage Users", complete_view, is_htmx)
        }
    }

    #[patch("/api/inline-edit/update-cell")]
    pub async fn update_cell(ctx: RequestContext) -> Response {
        let form = ctx.form.fields;

        let table_name = match form.get("table_to_modify") {
            Some(v) => v.to_string(),
            None => return Response::bad_request("Missing table identifier"),
        };
        let record_id = match form.get("id").and_then(|v| v.parse::<i64>().ok()) {
            Some(id) => id,
            None => return Response::bad_request("Invalid or missing record ID"),
        };
        let column_name = match form.get("column") {
            Some(col) => col.to_string(),
            None => return Response::bad_request("Missing targeted column metadata"),
        };
        let target_value = match form.get(&column_name) {
            Some(val) => val.to_string(),
            None => return Response::bad_request("Missing input field value update payload"),
        };

        let db = ctx.db.as_deref().unwrap();
        let user_repo = UserRepository { db: db.clone() };

        // ONE METHOD TO RULE THEM ALL: Automatically parses primitives, booleans, and dates!
        match user_repo
            .update_column_value(record_id, &column_name, target_value.clone())
            .await
        {
            Ok(updated_model) => {
                // 💡 Get the clean string representation straight back from the updated model
                let display_value = user_repo.get_field_as_string(&updated_model, &column_name);

                // Return component back to HTMX smoothly
                let single_input_html = html! {
                    input type="text"
                           value=(display_value)
                           hx-patch="/admin/api/inline-edit/update-cell"
                           hx-trigger="change"
                           name=(column_name)
                           hx-target="this"
                           hx-swap="outerHTML"
                           hx-vals=(format!("{{\"id\": {}, \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", record_id, column_name, table_name))
                           class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
                };

                Response::ok(single_input_html.into_string())
            }
            Err(err) => {
                // If parsing a datetime fails, it bubbles up cleanly as a sea_orm::DbErr::Custom
                Response::bad_request(format!("Database field rejection: {}", err))
            }
        }
    }

    #[get("/api/search-palette")]
    pub async fn command_palette_search(ctx: RequestContext) -> Response {
        let query = ctx
            .query
            .get("q")
            .map(|v| v.to_string().to_lowercase())
            .unwrap_or_default();

        if query.is_empty() {
            return Response::ok(""); // Return empty state if nothing is typed
        }

        let registry = ADMIN_REGISTRY.lock().unwrap();
        let mut matching_results = html! {};

        // Check for structural table navigation commands (e.g., typing "users" or "settings")
        for (table_name, meta) in registry.iter() {
            if table_name.contains(&query) {
                matching_results = html! {
                    (matching_results)
                    a href=(meta.route_path)
                       hx-get=(meta.route_path)
                       hx-target="#main-content"
                       class="flex items-center justify-between p-3 hover:bg-gray-800 rounded transition text-emerald-400" {
                           span class="text-gray-400" { "📋  Go to table: " }
                           span class="font-semibold" { (table_name) }
                    }
                };
            }
        }

        // 2. Fallback checking for general system views
        if "settings".contains(&query) || "system".contains(&query) {
            matching_results = html! {
                (matching_results)
                a href="/admin/settings" hx-get="/admin/settings" hx-target="#main-content"
                   class="flex items-center justify-between p-3 hover:bg-gray-800 rounded transition text-blue-400" {
                       span class="text-gray-400" { "⚙️ Action: " }
                       span class="font-semibold" { "System Settings" }
                }
            };
        }

        Response::ok(matching_results.into_string())
    }

    #[get("/users/search")]
    pub async fn search_users(ctx: RequestContext) -> Response {
        let db = ctx.db.as_deref().unwrap();
        let user_repo = UserRepository { db: db.clone() };
        let columns = UserRepository::column_names();
        let raw_table_name = user_repo.table_name();

        let query = ctx
            .query
            .get("q")
            .map(|v| Sanitizer::url_decode(v.as_str()))
            .unwrap_or_default();

        let users = if query.is_empty() {
            UserRepository::find()
                .order_by_desc(UserRepository::id_column())
                .all(db)
                .await
                .unwrap_or_default()
        } else {
            user_repo
                .search_admin_fields(&query)
                .await
                .unwrap_or_default()
        };

        // For plain query hits, treat as page 0, total_pages 1, total_items = matched length.
        // Set is_oob_update to TRUE so search events instantly update metrics live!
        let rendered_search_results = render_admin_table_view(
            &user_repo,
            &users,
            0,
            1,
            users.len() as u64,
            columns.len(),
            true, // Enable OOB Swap!
            Some(&query),
        );

        Response::ok(rendered_search_results)
    }
}

/// Unified admin data matrix rendering component.
/// Outputs matching dataset rows, infinite-scroll triggers, and real-time out-of-band statistical counters.
pub fn render_admin_table_view<R>(
    repo: &R,
    records: &[R::Model],
    page: u64,
    total_pages: u64,
    total_items: u64,
    columns_len: usize,
    is_oob_update: bool,
    search_query: Option<&str>,
) -> String
where
    R: ::gritshield::database::repository::GritRepository,
{
    let rows_html = html! {
        @if records.is_empty() {
            tr class="border-b border-gray-900" {
                td colspan=(columns_len) class="p-8 text-center text-gray-500 text-sm italic" {
                    @if let Some(q) = search_query {
                        "No records found matching \"" (q) "\""
                    } @else {
                        "No system entries found."
                    }
                }
            }
        } @else {
            @for record in records.iter() {
                tr class="divide-x divide-gray-800 hover:bg-gray-900/40 transition" {
                    @for col in repo.grid_columns().iter() {
                        td class="p-3 text-sm font-medium" {
                            @if col.is_editable {
                                // Editable Column Input Field
                                input type="text"
                                    value=(repo.get_field_as_string(record, &col.name))
                                    name=(col.name)
                                    hx-patch="/admin/api/inline-edit/update-cell"
                                    hx-trigger="change"
                                    hx-target="this"
                                    hx-swap="outerHTML"
                                    hx-vals=(format!(
                                        "{{\"id\": {}, \"column\": \"{}\", \"table_to_modify\": \"{}\"}}",
                                        repo.get_field_as_string(record, "id"), col.name, repo.table_name()
                                    ))
                                    class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
                            } @else {
                                // Read-only Text Column (e.g., id, updated_at)
                                span class="px-2 py-1 text-gray-400 font-mono text-xs" {
                                    (repo.get_field_as_string(record, &col.name))
                                }
                            }
                        }
                    }
                }
            }
        }

        // 1. Dynamic Infinite Scroll Trigger Row
        @if (page + 1) < total_pages {
            tr id="infinite-scroll-spinner"
                hx-get=(format!("/admin/{}?page={}", repo.table_name(), page + 1))
                hx-trigger="intersect once"
                hx-target="#infinite-scroll-spinner"
                hx-swap="outerHTML"
                class="border-t border-gray-900 bg-gray-950/50 animate-pulse"
            {
                td colspan=(columns_len) class="p-4" {
                    div class="flex items-center justify-center space-x-2" {
                        svg class="animate-spin h-4 w-4 text-emerald-500" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24" {
                            circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4" {}
                            path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" {}
                        }
                        span class="text-xs text-gray-400 font-medium" { "Loading more system entries..." }
                    }
                }
            }
        }

        // 2. Out-Of-Band (OOB) Statistical Update block targeting table footers
        @if is_oob_update {
            tr id="pagination-stats-target" hx-swap-oob="outerHTML" class="bg-gray-950 border-t border-gray-800 text-xs font-medium" {
                td colspan=(columns_len) class="p-4 bg-gray-950/90 backdrop-blur" {
                    div class="flex justify-between items-center w-full" {
                        span class="text-gray-400" {
                            "Viewing Page Slice " span class="text-emerald-400 font-mono font-semibold" { (&(page + 1)) }
                            " of " span class="text-gray-400 font-mono" { (total_pages) }
                        }
                        span class="text-xs text-gray-500 font-medium tracking-wide bg-gray-900 px-2.5 py-1 rounded-md border border-gray-800" {
                            "Total Matched Rows: " span class="text-gray-300 font-mono" { (total_items) }
                        }
                    }
                }
            }
        }
    };

    rows_html.into_string()
}
