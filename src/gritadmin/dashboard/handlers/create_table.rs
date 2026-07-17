use crate::database::repository::jql::DynamicColumnSpec;
use crate::gritadmin::dashboard::{error_response, success_response};
use crate::prelude::*;
use sea_orm::sea_query::{Alias, ColumnDef, Table};
use sea_orm::EntityTrait;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

pub async fn handle_create_table_dynamic(
    db: &DatabaseConnection,
    table_name: String,
    columns: Vec<DynamicColumnSpec>,
) -> Result<String, String> {
    let clean_name = table_name.trim().to_lowercase();
    if clean_name.is_empty() {
        return Err("Table name cannot be blank".to_string());
    }

    let backend = db.get_database_backend();

    // --- PRE-FLIGHT CHECK: Table Name Conflict Validation ---
    // Safely query database schemas based on your active driver protocol
    let table_exists = match backend {
        sea_orm::DatabaseBackend::Postgres => {
            let check_sql = sea_orm::Statement::from_string(
                backend,
                format!(
                    "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = '{}');",
                    clean_name
                ),
            );
            db.query_one(check_sql)
                .await
                .map(|res| {
                    res.map(|row| row.try_get_by_index::<bool>(0).unwrap_or(false))
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        }
        sea_orm::DatabaseBackend::MySql => {
            let check_sql = sea_orm::Statement::from_string(
                backend,
                format!(
                    "SELECT COUNT(*) FROM information_schema.tables WHERE table_name = '{}' AND table_schema = DATABASE();",
                    clean_name
                ),
            );
            db.query_one(check_sql)
                .await
                .map(|res| {
                    res.map(|row| row.try_get_by_index::<i64>(0).unwrap_or(0) > 0)
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        }
        sea_orm::DatabaseBackend::Sqlite => {
            let check_sql = sea_orm::Statement::from_string(
                backend,
                format!(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='{}';",
                    clean_name
                ),
            );
            db.query_one(check_sql)
                .await
                .map(|res| {
                    res.map(|row| row.try_get_by_index::<i32>(0).unwrap_or(0) > 0)
                        .unwrap_or(false)
                })
                .unwrap_or(false)
        }
    };

    if table_exists {
        // Return clear validation message to your API route wrapper
        return Err(format!(
            "Conflict: Table '{}' already exists in the database schema.",
            clean_name
        ));
    }
    // --------------------------------------------------------

    // Construct the standard query builder structure abstractly
    let mut stmt_builder = Table::create();
    stmt_builder
        .table(Alias::new(&clean_name))
        .if_not_exists()
        // Always enforce standard structural primary big integer identity
        .col(
            ColumnDef::new(Alias::new("id"))
                .big_integer()
                .not_null()
                .auto_increment()
                .primary_key(),
        );

    // Loop through dynamic choices requested via our developer UI panel
    for col in &columns {
        let col_name = col.name.trim().to_lowercase();
        if col_name.is_empty() || col_name == "id" {
            continue; // Skip invalid choices or explicit primary key overwrites
        }

        let mut column_definition = ColumnDef::new(Alias::new(col_name));

        // Map polymorphic type abstractions directly to standard native equivalents
        match col.r#type.as_str() {
            "int" => {
                column_definition.integer().not_null();
            }
            "bool" => {
                column_definition.boolean().not_null();
            }
            "datetime" => {
                column_definition.date_time().not_null();
            }
            "float" => {
                column_definition.float().not_null();
            }
            _ => {
                column_definition.string().not_null();
            } // Fallback default map string
        };

        stmt_builder.col(&mut column_definition);
    }

    // Turn abstract definitions seamlessly into engine-specific dialects
    let sql_statement = backend.build(&stmt_builder);

    match db.execute(sql_statement).await {
        Ok(_) => {
            // Automatically derive PascalCase for the Repository struct name without external crates
            let capitalized_repo_name = clean_name
                .split('_')
                .map(|word| {
                    let mut chars = word.chars();
                    match chars.next() {
                        None => String::new(),
                        Some(first_char) => {
                            first_char.to_uppercase().collect::<String>() + chars.as_str()
                        }
                    }
                })
                .collect::<String>();

            // Dynamic generation buffers for the model fields and annotations
            let mut model_fields_boilerplate = String::new();
            let mut grid_columns_list = vec![r#""id""#.to_string()];
            let mut searchable_columns_list = Vec::new();

            // All dynamic templates require a primary identity anchor field
            model_fields_boilerplate.push_str("    #[sea_orm(primary_key)]\n    pub id: i32,\n");

            // Scan attributes map to build out type systems and permissions
            for col in &columns {
                let rust_type = match col.r#type.as_str() {
                    "string" => {
                        searchable_columns_list.push(format!(r#""{}""#, col.name));
                        "String"
                    }
                    "int" => "i32",
                    "bool" => "bool",
                    "datetime" => "chrono::NaiveDateTime",
                    "float" => "f32",
                    _ => "String",
                };

                model_fields_boilerplate
                    .push_str(&format!("    pub {}: {},\n", col.name, rust_type));
                grid_columns_list.push(format!(r#""{}""#, col.name));
            }

            // Construct complete static code macro definition block
            let static_code_snippet = format!(
                r#"
// Add this boilerplate code block to your entities module

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, serde::Serialize, serde::Deserialize)]
#[sea_orm(table_name = "{}")]
pub struct Model {{
{}
}}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {{}}

impl ActiveModelBehavior for ActiveModel {{}}

// GritAdmin Repository Macro Definition

#[derive(GritAdmin)]
#[repository(
    searchable = [{}],
    grid_columns = [{}],
    read_only = ["id"]
)]
pub struct {}Repository;
"#,
                clean_name,
                model_fields_boilerplate,
                searchable_columns_list.join(", "),
                grid_columns_list.join(", "),
                capitalized_repo_name
            );

            // 5. Build standard styled markup payload for immediate display inside your workspace UI
            let ui_response_markup = format!(
                r#"<div class="space-y-4 animate-slide-in">
                    <div class="p-4 bg-emerald-950/20 border border-emerald-800/50 text-emerald-400 rounded-xl text-sm font-medium">
                        ✨ Successfully deployed live table <code>{}</code>.
                    </div>
                    
                    <div class="bg-gray-950 border border-gray-800 rounded-xl overflow-hidden shadow-2xl">
                        <div class="bg-gray-900 px-4 py-2 border-b border-gray-800 flex justify-between items-center">
                            <span class="text-xxs font-mono uppercase tracking-wider text-gray-500">GritAdmin Macro Architecture</span>
                            <button onclick="copyToClipboard(this, 'generated-code-block')"                        
                                class="text-xxs font-mono bg-gray-850 hover:bg-gray-800 text-gray-300 px-2 py-1 rounded transition border border-gray-700">
                                Copy
                            </button>
                        </div>
                        <pre id="generated-code-block" class="p-4 text-xs font-mono text-gray-300 overflow-x-auto selection:bg-emerald-800/40 select-text bg-gray-950/40"><code>{}</code></pre>
                    </div>
                </div>"#,
                clean_name,
                html_escape::encode_safe(&static_code_snippet)
            );

            Ok(ui_response_markup)
        }
        Err(e) => Err(format!("Database schema execution failure: {}", e)),
    }
}
