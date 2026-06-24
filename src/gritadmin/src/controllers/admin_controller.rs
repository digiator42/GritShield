use crate::{repository::admin_repository::UserRepository, shell};
use gritshield::{
    database::repository::{GritRepository, ADMIN_REGISTRY},
    prelude::*,
};
use sea_orm::{EntityTrait, PaginatorTrait, QueryOrder, QuerySelect, IntoActiveModel, ActiveModelTrait};


pub struct AdminUserController;

#[controller("/admin")]
impl AdminUserController {
    #[get("/users")]
    pub async fn list_users(ctx: RequestContext) -> Response {
        let is_htmx = ctx.req.has_header("hx-request");
        let db = ctx.db.as_deref().unwrap();
        let user_repo = UserRepository { db: db.clone() };

        // Parse the page query parameter (default to page 0)
        let page = ctx
            .query
            .get("page")
            .map(|v| v.parse::<u64>().unwrap())
            .unwrap_or(0);

        println!("====>> {}", page);

        let page_size = 15; // Number of spreadsheet rows per batch

        // Query page slice from database via Sea-ORM, sorted by newest ID
        // Note: crate::models::user::Entity is accessible via our repository associated types!
        let user_paginator = <UserRepository as GritRepository>::Entity::find()
            .order_by_desc(<UserRepository as GritRepository>::id_column())
            .paginate(db, page_size);

        let total_pages = user_paginator.num_pages().await.unwrap_or(0);
        let users = user_paginator.fetch_page(page).await.unwrap_or_default();

        // Trace block if it's completely empty to prevent silent failing
        if users.is_empty() && page == 0 {
            // Fallback: If pagination fails to fetch page 0 structurally, grab a standard raw slice limit
            // to make sure your spreadsheet layout doesn't render empty.
            println!("⚠️ Paginator returned empty slice on page 0. Falling back to default find sequence.");
            let fallback_users =
                <<UserRepository as GritRepository>::Entity as sea_orm::EntityTrait>::find()
                    .order_by_desc(<UserRepository as GritRepository>::id_column())
                    .limit(page_size)
                    .all(db)
                    .await
                    .unwrap_or_default();
            println!("{:?}", fallback_users);
        }

        // Render the dynamic spreadsheet row matrix
        let rows_html = html! {
            @for (index, user) in users.iter().enumerate() {
                @let is_last_row = index == (users.len() - 1) && (page + 1) < total_pages;

                // Use Maud's clean conditional attribute syntax:
                // [attribute] will only render if the variable inside is true!
                tr
                    hx-get=[is_last_row.then(|| format!("/admin/users?page={}", page + 1))]
                    hx-trigger=[is_last_row.then_some("intersect once")]
                    hx-swap=[is_last_row.then_some("afterend")]
                    class="divide-x divide-gray-800 hover:bg-gray-900/40 transition"
                {
                    td class="p-4 text-gray-500 font-mono text-xs" { (user.id) }
                    td class="p-3" {
                        input type="text"
                               value=(user.username)
                               hx-patch="/admin/api/inline-edit/update-cell"
                               hx-trigger="change"
                               name="username"
                               hx-target="this"
                               hx-swap="outerHTML"
                               hx-vals=(format!("{{\"id\": {}, \"column\": \"username\", \"table_to_modify\": \"users\"}}", user.id))
                               class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
                    }
                    td class="p-3" {
                        input type="text"
                               value=(user.email)
                               hx-patch="/admin/api/inline-edit/update-cell"
                               hx-trigger="change"
                               name="email"
                               hx-target="this"
                               hx-swap="outerHTML"
                               hx-vals=(format!("{{\"id\": {}, \"column\": \"email\", \"table_to_modify\": \"users\"}}", user.id))
                               class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
                    }
                }
            }
        };

        // If HTMX requested this, it's a scroll pagination hit!
        // Just return the raw incremental rows; HTMX swaps them right into place.
        if is_htmx {
            return Response::ok(rows_html.into_string());
        }

        // Otherwise, wrap the structural components inside the master dashboard envelope shell
        let complete_view = html! {
            div class="space-y-6" {
                div class="flex justify-between items-center" {
                    div {
                        h1 class="text-2xl font-bold tracking-tight" { "User Spreadsheet Matrix" }
                        p class="text-xs text-gray-500 mt-1" { "Double-click field inputs to execute reactive backend inline-edits dynamically." }
                    }

                    input type="text"
                           name="q"
                           placeholder="Type Alt+K to look up tables, or search records..."
                           hx-get="/admin/users/search"
                           hx-trigger="keyup changed delay:300ms"
                           hx-target="#table-body"
                           class="bg-gray-950 border border-gray-800 rounded px-4 py-2 w-80 text-sm focus:outline-none focus:border-emerald-500 transition";
                }

                div class="bg-gray-950 border border-gray-800 rounded-xl overflow-hidden shadow-xl" {
                    table class="w-full text-left border-collapse" {
                        thead class="bg-gray-900/80 backdrop-blur border-b border-gray-800 text-xs font-semibold uppercase tracking-wider text-gray-400" {
                            tr class="divide-x divide-gray-800" {
                                th class="p-4 w-20" { "Database ID" }
                                th class="p-4" { "Username String Field" }
                                th class="p-4" { "Registered Email Address" }
                            }
                        }
                        tbody id="table-body" class="divide-y divide-gray-800" {
                            (rows_html)
                        }
                    }
                }
            }
        };

        shell::admin_shell("Manage Users", complete_view, is_htmx)
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

        // Find the record from the database
        if let Some(user) = user_repo.find_by_id(record_id).await.unwrap() {

            // 1. Convert via standard IntoActiveModel to preserve database primary key context
            let mut active_user = user.into_active_model();

            // 2. Change ONLY the targeted column to Set (dirty)
            match column_name.as_str() {
                "username" => active_user.username = sea_orm::Set(target_value.clone()),
                "email" => active_user.email = sea_orm::Set(target_value.clone()),
                _ => return Response::bad_request("Unknown column field cluster"),
            };

            // 3. Execute the update directly using Sea-ORM's active model trait
            active_user.update(db).await.unwrap();

            // 4. Return the component back to HTMX
            let single_input_html = html! {
                input type="text"
                       value=(target_value)
                       hx-patch="/admin/api/inline-edit/update-cell"
                       hx-trigger="change"
                       name=(column_name)
                       hx-target="this"
                       hx-swap="outerHTML"
                       hx-vals=(format!("{{\"id\": {}, \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", record_id, column_name, table_name))
                       class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
            };

            Response::ok(single_input_html.into_string())
        } else {
            Response::not_found("Record missing")
        }
    }

    #[get("/api/search-palette")]
    pub async fn command_palette_search(ctx: RequestContext) -> Response {
        let query = ctx
            .query
            .get("q")
            .map(|v| v.to_string().to_lowercase())
            .unwrap_or_default();

        println!("===> {}", query);

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
}
