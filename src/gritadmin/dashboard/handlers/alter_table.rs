use crate::gritadmin::dashboard::{error_response, success_response};
use crate::prelude::*;
use sea_orm::sea_query::{Alias, ColumnDef, Table};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};


pub async fn handle_append_table_column(
    db: &DatabaseConnection,
    table_name: &str,
    raw_col_name: &str,
    col_type: &str,
) -> Result<String, String> {
    let clean_table = table_name.trim().to_lowercase();
    let clean_col = raw_col_name.trim().to_lowercase(); //.replace(/[^a-z0-9_]/g, "");

    if clean_col.is_empty() || clean_col == "id" {
        return Err("Invalid or restricted column name choice.".to_string());
    }

    let backend = db.get_database_backend();

    // Build an abstract SeaORM Alter Table statement
    let mut alter_table = sea_orm::sea_query::Table::alter();
    alter_table.table(sea_orm::sea_query::Alias::new(&clean_table));

    let mut column_definition = ColumnDef::new(sea_orm::sea_query::Alias::new(&clean_col));

    // Map your custom polymorphic frontend abstractions cleanly to native equivalents
    match col_type {
        "int" => {
            column_definition.integer().null();
        }
        "bool" => {
            column_definition.boolean().null();
        }
        "datetime" => {
            column_definition.date_time().null();
        }
        "float" => {
            column_definition.float().null();
        }
        _ => {
            column_definition.string().null();
        }
    };

    alter_table.add_column(&mut column_definition);
    let sql_statement = backend.build(&alter_table);

    // Execute directly against the engine runtime
    match db.execute(sql_statement).await {
        Ok(_) => Ok(format!(
            "Successfully appended column '{}' ({}) to table '{}'. Make sure to add it to your struct entity file!",
            clean_col, col_type, clean_table
        )),
        Err(e) => Err(format!("Schema modification failed: {}", e)),
    }
}

// POST /admin/api/alter-table/:table_slug/add-column
pub async fn alter_table_add_column_handler(ctx: RequestContext) -> Response {
    let table_slug = ctx.params.get("table_slug").unwrap().as_str();

    let col_name = ctx.form.fields.get("column_name").and_then(|v| v.first()).map(|v| v.as_str()).unwrap_or("");
    let col_type = ctx.form.fields.get("column_type").and_then(|v| v.first()).map(|v| v.as_str()).unwrap_or("");

    if col_name.trim().is_empty() {
        return error_response("Column name input parameter field cannot be empty.");
    }

    match handle_append_table_column(&ctx.db.unwrap().as_ref(), &table_slug, &col_name, &col_type)
        .await
    {
        Ok(success_msg) => {
            // Fires an empty body 200 payload with your fixed 'hx-trigger' success toast header
            success_response(success_msg)
        }
        Err(err_msg) => error_response(err_msg),
    }
}
