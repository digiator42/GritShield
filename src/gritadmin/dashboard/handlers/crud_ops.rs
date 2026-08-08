use crate::database::repository::jql::DynamicColumnSpec;
use crate::database::repository::registry::ADMIN_REGISTRY;
use crate::database::repository::GritRepository;
use crate::gritadmin::dashboard::{
    error_response, render_grid_rows, render_results_grid, success_response,
};
use crate::gritadmin::handle_list;
use crate::prelude::*;
use crate::security::xss::{Sanitizer, UntrustedString};
use maud::html;
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, PaginatorTrait, QueryOrder,
    Statement, TransactionTrait,
};
use std::collections::HashMap;

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
        .and_then(|v| v.first())
        .map(|v| v.as_str())
        .or_else(|| ctx.query.get("id").and_then(|v| v.first()).map(|v| v.as_str()))
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
    let record_id_raw = match form.get("id").and_then(|v| v.first()) {
        Some(id) => id,
        None => return error_response("Missing record ID"),
    };
    let record_id = match record_id_raw.parse::<<R as GritRepository>::Id>() {
        Ok(id) => id,
        Err(e) => return error_response(format!("Invalid record ID: {}", e)),
    };
    let raw_column = match form.get("column").and_then(|v| v.first()) {
        Some(col) => col.as_str(),
        None => return error_response("Missing targeted column"),
    };
    let column_name = Sanitizer::url_decode(raw_column);
    let raw_value = match form.get(raw_column).and_then(|v| v.first()) {
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
                    hx-vals=(format!("{{\"id\": \"{}\", \"column\": \"{}\", \"table_to_modify\": \"{}\"}}", record_id_raw.as_str(), column_name, table_slug))
                    class="bg-transparent hover:bg-gray-850 focus:bg-gray-800 px-2 py-1 rounded focus:outline-none w-full border border-transparent focus:border-emerald-600 transition";
            };
            Response::ok(single_input_html.into_string())
        }
        Err(e) => error_response(format!("Database field rejection: {}", e)),
    }
}

pub async fn handle_bulk_create_modal<R>(
    _ctx: RequestContext,
    repo: R,
    table_slug: &'static str,
) -> Response
where
    R: GritRepository + Send + Sync + 'static,
{
    // Auto-generate a helper blueprint from the exact editable columns registered in the grid
    let template_fields: Vec<String> = repo
        .grid_columns()
        .iter()
        .map(|c| format!("\"{}\": \"value\"", c.name))
        .collect();

    let json_placeholder = format!("[\n  {{\n    {}\n  }}\n]", template_fields.join(",\n    "));

    let modal_html = html! {
        div id="bulk-modal" class="fixed inset-0 bg-black/80 backdrop-blur-sm flex items-center justify-center z-40 animate-fade-in" onclick="if(event.target === this) this.remove()" {
            div class="bg-gray-950 border border-gray-800 rounded-xl w-full max-w-2xl overflow-hidden shadow-2xl flex flex-col max-h-[85vh]" onclick="event.stopPropagation()" {

                // Header
                div class="p-4 border-b border-gray-800 bg-gray-900/50 flex justify-between items-center" {
                    h3 class="text-sm font-mono font-bold text-gray-200" { "Bulk Record Ingestion Console :: " (table_slug) }
                    button class="text-gray-500 hover:text-gray-400 font-mono text-xs" onclick="document.getElementById('bulk-modal').remove()" { "✕" }
                }

                // Form Body
                form hx-post=(format!("/admin/{}/bulk-create", table_slug))
                     hx-target="#matrix-wrapper"
                     hx-vals="{\"partial\": \"matrix\"}"
                     hx-indicator="body"
                     hx-on--after-request="if(event.detail.successful) document.getElementById('bulk-modal').remove()"
                     class="flex-1 flex flex-col overflow-hidden p-4 space-y-4 overflow-scroll" {

                    div class="bg-gray-900/40 p-3 rounded-lg border border-gray-800/60" {
                        span class="text-[11px] font-mono text-gray-400 block mb-1.5" { "💡 Ingestion Matrix Layout (Copy & Adapt):" }
                        pre class="text-[11px] font-mono text-emerald-500 bg-black/60 p-2 rounded overflow-x-auto border border-emerald-950/40 select-all" {
                            (json_placeholder)
                        }
                    }

                    div class="flex-1 flex flex-col min-h-[280px]" {
                        label class="text-xs font-mono text-gray-300 mb-1" { "Payload Data Stream:" }
                        textarea
                            name="bulk_json"
                            placeholder="[ { ... }, { ... } ]"
                            class="flex-1 w-full bg-black/40 border border-gray-800 rounded-lg p-3 font-mono text-xs text-gray-300 focus:outline-none focus:border-emerald-600 transition resize-none placeholder-gray-700"
                            required {}
                    }

                    // Bottom Action Panel
                    div class="flex justify-end items-center gap-2 pt-2 border-t border-gray-900" {
                        button type="button"
                                onclick="document.getElementById('bulk-modal').remove()"
                                class="px-3 py-2 text-xs font-mono text-gray-400 hover:text-gray-300 transition" {
                            "Cancel"
                        }
                        button type="submit"
                                class="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 text-white font-mono text-xs rounded-lg shadow-lg shadow-emerald-900/20 transition" {
                            "⚡ Commit Ingestion"
                        }
                    }
                }
            }
        }
    };

    Response::new(200, Sanitizer::trust(&modal_html.into_string()))
}
pub async fn handle_bulk_create<R>(
    mut ctx: RequestContext,
    repo: R,
    table_slug: &'static str,
) -> Response
where
    R: GritRepository + Send + Sync + 'static,
    // Handle model to active model mapping constraints
    <R as GritRepository>::Model: Sync
        + Send
        + sea_orm::IntoActiveModel<<<R as GritRepository>::Entity as EntityTrait>::ActiveModel>,
    // Handle serde deserialization limits for the payload
    for<'de> <R as GritRepository>::Model: serde::Deserialize<'de>,
    // Enforce that the hydrated ActiveModel can be passed across threads safely
    <<R as GritRepository>::Entity as EntityTrait>::ActiveModel: std::marker::Send,
    <R as GritRepository>::Id: std::str::FromStr,
    <<R as GritRepository>::Id as std::str::FromStr>::Err: std::fmt::Display,
{
    use sea_orm::{ActiveModelTrait, EntityTrait, TransactionTrait};

    // Unpack incoming form payload
    let payload_str = match ctx.form.fields.get("bulk_json").and_then(|v| v.first()) {
        Some(val) => Sanitizer::url_decode(val.as_str()),
        None => {
            return error_response("Missing key field 'bulk_json' inside the request body payload")
        }
    };

    // Validate valid JSON structure
    let json_array: serde_json::Value = match serde_json::from_str(&payload_str) {
        Ok(val) => val,
        Err(e) => return error_response(format!("Invalid structural JSON format: {}", e)),
    };

    let items_array = match json_array.as_array() {
        Some(arr) => arr,
        None => {
            return error_response(
                "Ingestion root wrapper is required to be an explicit Array block: [ ... ]",
            )
        }
    };

    if items_array.is_empty() {
        return error_response("Ingestion stream data collection cannot be empty");
    }

    let db = repo.get_db();

    // Spool up an isolated ACID transaction boundary
    let txn = match db.begin().await {
        Ok(t) => t,
        Err(e) => {
            return error_response(format!(
                "Failed to configure safe execution context transaction: {}",
                e
            ))
        }
    };

    let mut records_processed = 0;

    for (idx, json_obj) in items_array.iter().enumerate() {
        let mut item_map = match json_obj.as_object() {
            Some(map) => map.clone(),
            None => {
                return error_response(format!(
                    "Malformatted item block encountered at position index ({})",
                    idx
                ))
            }
        };

        // =====================================================================
        // Step A: Primary Key Assignment Optimization
        // =====================================================================
        let mut primary_key_needs_reset = false;
        if let Some(id_val) = item_map.get("id") {
            if id_val.as_i64() == Some(0) {
                primary_key_needs_reset = true;
            }
        } else {
            // Fallback injection if the developer omitted it entirely
            item_map.insert("id".to_string(), serde_json::json!(0));
            primary_key_needs_reset = true;
        }

        // =====================================================================
        // Step B: Dynamic Date Normalization (Mimicking update cell parsing)
        // =====================================================================
        for (_field_name, value) in item_map.iter_mut() {
            if let serde_json::Value::String(ref mut field_str) = value {
                // Intercept standard human/SQL timestamp variations: "YYYY-MM-DD HH:MM:SS"
                // Check format lengths and matching indices safely to avoid panics
                if field_str.len() >= 19
                    && field_str.as_bytes()[4] == b'-'
                    && field_str.as_bytes()[7] == b'-'
                    && field_str.as_bytes()[10] == b' '
                {
                    // Convert space separation directly to standard ISO-8601 'T' delimiter
                    *field_str = field_str.replace(" ", "T");
                }
            }
        }

        // Wrap back to a structured Value block
        let cleaned_json = serde_json::Value::Object(item_map);

        // Hydrate the ActiveModel natively (Serde passes safely now)
        let mut active_model =
            match <<R as GritRepository>::Entity as EntityTrait>::ActiveModel::from_json(
                cleaned_json,
            ) {
                Ok(am) => am,
                Err(e) => {
                    return error_response(format!(
                        "Row hydration mapping constraints rejected at row index {}: {}",
                        idx, e
                    ));
                }
            };

        // Un-set primary key if placeholder was 0 to trigger native sequences
        if primary_key_needs_reset {
            use sea_orm::{Iterable, PrimaryKeyToColumn};

            for pk_variant in <<R as GritRepository>::Entity as EntityTrait>::PrimaryKey::iter() {
                active_model.not_set(pk_variant.into_column());
            }
        }

        // Insert cleanly inside the isolated transaction guard
        if let Err(e) = active_model.insert(&txn).await {
            return error_response(format!(
                "Database schema violation encountered at row index {}: {}",
                idx, e
            ));
        }
        records_processed += 1;
    }

    // Commit structural changes permanently to storage
    if let Err(e) = txn.commit().await {
        return error_response(format!("Failed finalizing batch commit operation: {}", e));
    }

    crate::debug!(
        "[BULK IMPORT] ✓ Successfully committed {} entries into {}",
        records_processed,
        table_slug
    );

    // Inject HTMX-driven interface refresh using the matrix partial layout
    ctx.query.insert(
        "partial".to_string(),
        vec![UntrustedString::new("matrix".to_string())],
    );
    let mut refreshed_view = handle_list(ctx, repo, table_slug).await;

    // Drop a beautiful UI toast directly over HTMX custom event headers
    let success_toast = format!(
        r#"{{"showToast": {{"message": "Successfully ingested {} rows into {}!", "type": "success"}}}}"#,
        records_processed, table_slug
    );
    refreshed_view
        .headers
        .push(("hx-trigger".to_string(), success_toast));

    refreshed_view
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
    let ids_str = match ctx.form.fields.get("ids").and_then(|v| v.first()) {
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
