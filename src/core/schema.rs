use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use lazy_static::lazy_static;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RelationKind {
    HasMany,
    HasOne,
    BelongsTo,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FieldSchema {
    pub name: String,
    pub type_: String,        // e.g., "String", "i32", "NaiveDateTime"
    pub nullable: bool,
    pub primary_key: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RelationSchema {
    pub kind: RelationKind,
    pub target_table: String,
    pub foreign_key: Option<String>,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ModelSchema {
    pub table_name: String,
    pub fields: Vec<FieldSchema>,
    pub relations: Vec<RelationSchema>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RepositorySchema {
    pub table_name: String,
    pub grid_columns: Vec<String>,
    pub searchable_columns: Vec<String>,
    pub read_only_columns: Vec<String>,
    pub route_path: String,
}

lazy_static! {
    static ref SCHEMA_REGISTRY: Arc<Mutex<HashMap<String, ModelSchema>>> =
        Arc::new(Mutex::new(HashMap::new()));
    static ref REPOSITORY_REGISTRY: Arc<Mutex<HashMap<String, RepositorySchema>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

pub fn register_model_schema(table_name: &str, fields: Vec<FieldSchema>, relations: Vec<RelationSchema>) {
    let mut registry = SCHEMA_REGISTRY.lock().unwrap();
    let entry = registry.entry(table_name.to_string()).or_insert_with(|| ModelSchema {
        table_name: table_name.to_string(),
        fields: Vec::new(),
        relations: Vec::new(),
    });
    entry.fields = fields;
    entry.relations = relations;
}

pub fn add_relations(table_name: &str, relations: Vec<RelationSchema>) {
    let mut registry = SCHEMA_REGISTRY.lock().unwrap();
    if let Some(entry) = registry.get_mut(table_name) {
        entry.relations.extend(relations);
    } else {
        let mut schema = ModelSchema {
            table_name: table_name.to_string(),
            fields: Vec::new(),
            relations: Vec::new(),
        };
        schema.relations = relations;
        registry.insert(table_name.to_string(), schema);
    }
}

pub fn register_repository(table_name: &str, repo: RepositorySchema) {
    let mut registry = REPOSITORY_REGISTRY.lock().unwrap();
    registry.insert(table_name.to_string(), repo);
}

pub fn export_openapi(output_path: &str) -> Result<(), std::io::Error> {
    use std::fs::File;
    use std::io::Write;

    let model_registry = SCHEMA_REGISTRY.lock().unwrap();
    let repo_registry = REPOSITORY_REGISTRY.lock().unwrap();

    // Merge: combine model and repository info
    let mut combined = serde_json::Map::new();
    for (table, model) in model_registry.iter() {
        let mut obj = serde_json::to_value(model).unwrap().as_object().unwrap().clone();
        if let Some(repo) = repo_registry.get(table) {
            obj.insert("repository".to_string(), serde_json::to_value(repo).unwrap());
        }
        combined.insert(table.clone(), serde_json::Value::Object(obj));
    }

    // Add any repository-only entries (models without a schema? unlikely)
    for (table, repo) in repo_registry.iter() {
        if !combined.contains_key(table) {
            let mut obj = serde_json::Map::new();
            obj.insert("repository".to_string(), serde_json::to_value(repo).unwrap());
            combined.insert(table.clone(), serde_json::Value::Object(obj));
        }
    }

    let json = serde_json::to_string_pretty(&combined)?;
    let mut file = File::create(output_path)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}