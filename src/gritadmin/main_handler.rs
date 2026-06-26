use crate::database::repository::GritRepository;
use crate::deps::sea_orm::{EntityTrait, PaginatorTrait, QueryOrder};
use crate::{admin_shell, prelude::*};
use maud::{html, Markup};

/// Generic dashboard view runner for listing data rows and handling infinite scrolls.
pub async fn handle_list<R>(ctx: RequestContext, repo: R, table_slug: &'static str) -> Response
where
    R: GritRepository + Send + Sync + 'static, 
    <R as GritRepository>::Model: Sync + Send,
{
    let is_htmx = ctx.req.has_header("hx-request");
    let db = repo.get_db();

    // Parse requested pagination frame page
    let page = ctx
        .query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(0);
    let is_infinite_scroll = is_htmx && page > 0;
    let page_size = 15;

    // Execute paginated selection queries generically
    let paginator = <R::Entity as EntityTrait>::find()
        .order_by_desc(R::id_column())
        .paginate(db, page_size);

    let total_pages = paginator.num_pages().await.unwrap_or(0);
    let items = paginator.fetch_page(page).await.unwrap_or_default();

    let route_path_str = format!("/admin/{}", table_slug);
    let route_patch_str = format!("/admin/{}/update-cell", table_slug);

    let rows_html = html! {
        @for item in items.iter() {
            tr class="divide-x divide-gray-800 hover:bg-gray-900/40 transition" {
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
                                hx-vals=(format!("{{\"id\": {}, \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", repo.get_field_as_string(item, "id"), col.name, table_slug))
                                class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
                        } @else {
                            span class="px-2 py-1 text-gray-400 font-mono text-xs" { (repo.get_field_as_string(item, &col.name)) }
                        }
                    }
                }
            }
        }
        @if (page + 1) < total_pages {
            tr id="infinite-scroll-spinner"
                hx-get=(format!("{}?page={}", route_path_str, page + 1))
                hx-trigger="intersect once"
                hx-target="#infinite-scroll-spinner"
                hx-swap="outerHTML"
                class="border-t border-gray-900 bg-gray-950/50 animate-pulse" {
                td colspan=(repo.grid_columns().len()) class="p-4 text-center" {
                    span class="text-xs text-gray-400 font-medium" { "Loading more records..." }
                }
            }
        }
    };

    if is_infinite_scroll {
        Response::ok(rows_html.into_string())
    } else {
        let display_title = format!("{} Workspace Matrix", table_slug.to_uppercase());
        let route_search_str = format!("/admin/{}/search", table_slug);

        let complete_view = html! {
            div class="space-y-6" {
                div class="flex justify-between items-center" {
                    h1 class="text-2xl font-bold tracking-tight" { (display_title) }
                    input type="text"
                        name="q"
                        placeholder="Search records..."
                        hx-get=(route_search_str)
                        hx-trigger="keyup changed delay:300ms"
                        hx-target="#table-body"
                        class="bg-gray-950 border border-gray-800 rounded px-4 py-2 w-80 text-sm focus:outline-none focus:border-emerald-500 transition";
                }
                div class="bg-gray-950 border border-gray-800 rounded-xl overflow-hidden shadow-xl" {
                    table class="w-full text-left border-collapse" {
                        thead class="bg-gray-900/80 border-b border-gray-800 text-xs font-semibold uppercase tracking-wider text-gray-400" {
                            tr class="divide-x divide-gray-800" {
                                @for col in repo.grid_columns().iter() { th class="p-4" { (col.label) } }
                            }
                        }
                        tbody id="table-body" class="divide-y divide-gray-800" { (rows_html) }
                    }
                }
            }
        };
        admin_shell(&display_title, complete_view, is_htmx)
    }
}

/// Generic search query processor handling dynamic query filters.
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

    let rows_html = html! {
        @for item in items.iter() {
            tr class="divide-x divide-gray-800 hover:bg-gray-900/40 transition" {
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
                                hx-vals=(format!("{{\"id\": {}, \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", repo.get_field_as_string(item, "id"), col.name, table_slug))
                                class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
                        } @else {
                            span class="px-2 py-1 text-gray-400 font-mono text-xs" { (repo.get_field_as_string(item, &col.name)) }
                        }
                    }
                }
            }
        }
    };
    Response::ok(rows_html.into_string())
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

    // 1. Get the raw string reference first
    let record_id_raw = match form.get("id") {
        Some(id) => id,
        None => return Response::bad_request("Missing record ID"),
    };

    // 2. Parse directly into the repository's native ID type
    let record_id = match record_id_raw.parse::<<R as GritRepository>::Id>() {
        Ok(id) => id,
        Err(err) => return Response::bad_request(format!("Invalid record ID format: {}", err)),
    };

    let column_name = match form.get("column") {
        Some(col) => col.to_string(),
        None => return Response::bad_request("Missing targeted column"),
    };
    let target_value = match form.get(&column_name) {
        Some(val) => val.to_string(),
        None => return Response::bad_request("Missing field update payload"),
    };

    let route_patch_str = format!("/admin/{}/update-cell", table_slug);

    // 3. Pass the correctly typed record_id here
    match repo
        .update_column_value(record_id, &column_name, target_value)
        .await
    {
        Ok(updated_model) => {
            let display_value = repo.get_field_as_string(&updated_model, &column_name);
            let single_input_html = html! {
                input type="text"
                    value=(display_value)
                    hx-patch=(route_patch_str)
                    hx-trigger="change"
                    name=(column_name)
                    hx-target="this"
                    hx-swap="outerHTML"
                    // Enclose id value in quotes so it handles both integer keys and UUID strings safely
                    hx-vals=(format!("{{\"id\": \"{}\", \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", record_id_raw, column_name, table_slug))
                    class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
            };
            Response::ok(single_input_html.into_string())
        }
        Err(err) => Response::bad_request(format!("Database field rejection: {}", err)),
    }
}
