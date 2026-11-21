use crate::database::repository::registry::ADMIN_REGISTRY;

/// Check if a column name looks like a foreign key.
pub fn is_foreign_key_column(col_name: &str) -> bool {
    col_name.ends_with("_id")
}

fn slug_for_table(target: &str) -> Option<String> {
    let admin_reg = ADMIN_REGISTRY.lock().ok()?;

    if let Some(meta) = admin_reg.get(target) {
        return Some(meta.table_slug.to_string());
    }
    if let Some((_, meta)) = admin_reg.iter().find(|(_, m)| m.table_name == target) {
        return Some(meta.table_slug.to_string());
    }

    let plural = format!("{}s", target);
    if let Some(meta) = admin_reg.get(plural.as_str()) {
        return Some(meta.table_slug.to_string());
    }

    if target.ends_with('s') && target.len() > 1 {
        let singular = &target[..target.len() - 1];
        if let Some(meta) = admin_reg.get(singular) {
            return Some(meta.table_slug.to_string());
        }
    }

    None
}

/// Try to resolve the target table slug for a foreign key column using the Schema Registry.
pub fn get_target_table_slug(current_table: &str, col_name: &str) -> Option<String> {
    let normalized_col = col_name.replace("_", "").to_lowercase();

    let real_table = ADMIN_REGISTRY
        .lock()
        .ok()
        .and_then(|reg| {
            reg.iter()
                .find(|(_, m)| m.table_slug == current_table || m.table_name == current_table)
                .map(|(_, m)| m.table_name.to_string())
        })
        .unwrap_or_else(|| current_table.to_string());

    if let Ok(schema_reg) = crate::core::schema::SCHEMA_REGISTRY.lock() {
        let keys = [
            current_table.to_string(),
            real_table.clone(),
            format!("{}s", current_table),
            format!("{}s", real_table),
        ];

        for key in keys.iter() {
            if let Some(model_schema) = schema_reg.get(key.as_str()) {
                for relation in &model_schema.relations {
                    if relation.kind == crate::core::schema::RelationKind::BelongsTo {
                        if let Some(ref fk) = relation.foreign_key {
                            if fk.replace("_", "").to_lowercase() == normalized_col {
                                let target = &relation.target_table;
                                return Some(
                                    slug_for_table(target).unwrap_or_else(|| target.clone()),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    if !col_name.ends_with("_id") {
        return None;
    }

    let base = col_name.trim_end_matches("_id");
    slug_for_table(base)
}
