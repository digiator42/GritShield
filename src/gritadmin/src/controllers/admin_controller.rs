use crate::{repository::admin_repository::UserRepository, shell};
use gritshield::{database::repository::{ADMIN_REGISTRY, GritRepository}, prelude::*};

pub struct AdminUserController;

#[controller("/admin")]
impl AdminUserController {
    #[get("/users")]
    pub async fn list_users(ctx: RequestContext) -> Response {
        let is_htmx = ctx.req.has_header("hx-request");

        // Mock template rendering for user records data view grid
        let view = html! {
            div class="space-y-6" {
                div class="flex justify-between items-center" {
                    h1 class="text-2xl font-bold" { "User Registry Spreadsheet" }

                    // Unified Search Bar: Fires an AJAX search request 300ms after user stops typing
                    input type="text"
                           name="q"
                           placeholder="Type Cmd+K or search records..."
                           hx-get="/admin/users/search"
                           hx-trigger="keyup changed delay:300ms"
                           hx-target="#table-body"
                           class="bg-gray-800 border border-gray-700 rounded px-4 py-2 w-80 focus:outline-none focus:border-emerald-500";
                }

                // Interactive Spreadsheet Data Matrix Table Grid Layout
                div class="bg-gray-950 border border-gray-800 rounded-lg overflow-hidden" {
                    table class="w-full text-left border-collapse" {
                        thead class="bg-gray-900 border-b border-gray-800" {
                            tr {
                                th class="p-4" { "ID" }
                                th class="p-4" { "Username (Double Click to Inline Edit)" }
                                th class="p-4" { "Email Space" }
                            }
                        }
                        tbody id="table-body" class="divide-y divide-gray-800" {
                            // Example of an inline cell field hook
                            tr {
                                td class="p-4 text-gray-500" { "1" }
                                td class="p-4 cursor-pointer hover:bg-gray-800 transition" {
                                    input type="text"
                                           value="admin_user"
                                           hx-patch="/admin/api/inline-edit"
                                           hx-trigger="change"
                                           name="username"
                                           class="bg-transparent focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full";
                                }
                                td class="p-4" { "admin@gritshield.io" }
                            }
                        }
                    }
                }
            }
        };

        shell::admin_shell("Manage Users", view, is_htmx)
    }

    #[patch("/api/inline-edit/update-cell")]
    pub async fn update_cell(ctx: RequestContext) -> Response {
        // 1. Grab raw payload safely using your structural types
        let form = ctx.form.fields;
        let table_name = form.get("table_to_modify").map(|v| v.to_string()).unwrap(); // e.g., "users"
        let record_id = form.get("id").map(|v| v.parse::<i64>().unwrap()).unwrap(); // Test UntrustedString::parse!
        let column_name = form.get("column").map(|v| v.to_string()).unwrap(); // e.g., "username"
        let target_value = form.get("value").map(|v| v.to_string()).unwrap(); // The raw untrusted text input

        // 2. Fetch the existing record through your repository suite
        let db = ctx.db.as_deref().unwrap();
        let user_repo = UserRepository { db: db.clone() };
        if let Some(mut user) = user_repo.find_by_id(record_id).await.unwrap() {
            // 3. Update the specific field dynamically
            match column_name.as_str() {
                "username" => user.username = target_value.to_string(),
                "email" => user.email = target_value.to_string(),
                _ => return Response::bad_request("Unknown column field cluster"),
            };

            // 4. Fire your JPA-style update function!
            user_repo.save(user).await.unwrap();
            Response::ok("Cell synchronized successfully")
        } else {
            Response::not_found("Record missing")
        }
    }

    #[get("/api/search-palette")]
    pub async fn command_palette_search(ctx: RequestContext) -> Response {
        let query = ctx
            .form
            .fields
            .get("q")
            .map(|v| v.to_string().to_lowercase())
            .unwrap_or_default();

        if query.is_empty() {
            return Response::ok(""); // Return empty state if nothing is typed
        }

        let registry = ADMIN_REGISTRY.lock().unwrap();
        let mut matching_results = html! {};

        // 1. Check for structural table navigation commands (e.g., typing "users" or "settings")
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
