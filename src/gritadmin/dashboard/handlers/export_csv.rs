use crate::database::repository::GritRepository;
use crate::database::GridColumn;
use crate::deps::sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, PaginatorTrait, QueryOrder,
    Statement, TransactionTrait,
};
use crate::gritadmin::dashboard::error_response;
use crate::prelude::*;
use sea_orm::sea_query::{Alias, ColumnDef, Table};
use sea_orm::ColumnTrait;
use sea_orm::QueryFilter;
use sea_orm::QueryResult;
use std::collections::HashMap;
use std::fmt::Write;

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
        let str_value = value.first().map(|v| v.as_str().to_string()).unwrap_or_default();
        if let Some(stripped) = key.strip_prefix("filter__") {
            if let Some(rest) = stripped.strip_suffix("__op") {
                op_map.insert(rest.to_string(), str_value);
            } else if let Some(rest) = stripped.strip_suffix("__value") {
                val_map.insert(rest.to_string(), str_value);
            }
        } else if key == "q" {
            search_q = Some(str_value);
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

/// Simple CSV writer (escapes commas and quotes).
pub fn export_to_csv<R>(records: &[R::Model], columns: &[GridColumn], repo: &R) -> String
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
