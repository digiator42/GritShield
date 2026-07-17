use crate::database::repository::GritRepository;
use crate::gritadmin::dashboard::{error_response};
use crate::gritadmin::shell;
use crate::gritadmin::models::audit_log;
use sea_orm::QueryOrder;
use crate::prelude::*;
use maud::html;
use sea_orm::EntityTrait;
use sea_orm::QueryFilter;
use sea_orm::ColumnTrait;

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

    // Fetch audit logs for this record
    let logs = audit_log::Entity::find()
        .filter(audit_log::Column::TableName.eq(repo.table_name()))
        .filter(audit_log::Column::RecordId.eq(id_str))
        .order_by_desc(audit_log::Column::Timestamp)
        .all(repo.get_db())
        .await
        .unwrap_or_default();

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
        shell::admin_shell(&title, detail_html, false)
    }
}
