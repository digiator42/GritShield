use crate::database::repository::registry::ADMIN_REGISTRY;
use crate::core::schema::SCHEMA_REGISTRY;

/// Check if a column name looks like a foreign key.
pub fn is_foreign_key_column(col_name: &str) -> bool {
    col_name.ends_with("_id")
}

/// Try to resolve the target table slug for a foreign key column using the Schema Registry.
pub fn get_target_table_slug(current_table: &str, col_name: &str) -> Option<String> {
    // Try to find an explicit relation from the SCHEMA_REGISTRY
    if let Ok(schema_reg) = crate::core::schema::SCHEMA_REGISTRY.lock() {
        if let Some(model_schema) = schema_reg.get(current_table) {
            // Normalize column name to ignore underscores and case (e.g., "follower_id" -> "followerid")
            let normalized_col = col_name.replace("_", "").to_lowercase();

            for relation in &model_schema.relations {
                if relation.kind == crate::core::schema::RelationKind::BelongsTo {
                    if let Some(ref fk) = relation.foreign_key {
                        let normalized_fk = fk.replace("_", "").to_lowercase();

                        // Match found (e.g., "followerid" == "followerid")
                        if normalized_fk == normalized_col {
                            let target = &relation.target_table;

                            // Check if the target table exists in ADMIN_REGISTRY,
                            // or fallback to singular/plural variants if there's a mismatch
                            if let Ok(admin_reg) = ADMIN_REGISTRY.lock() {
                                if admin_reg.contains_key(target.as_str()) {
                                    return Some(target.clone());
                                }
                                let plural = format!("{}s", target);
                                if admin_reg.contains_key(plural.as_str()) {
                                    return Some(plural);
                                }
                                if target.ends_with('s') && target.len() > 1 {
                                    let singular = &target[..target.len() - 1];
                                    if admin_reg.contains_key(singular) {
                                        return Some(singular.to_string());
                                    }
                                }
                            }
                            return Some(target.clone());
                        }
                    }
                }
            }
        }
    }

    // Fallback heuristic for standard naming conventions if no registry relation matches
    if !col_name.ends_with("_id") {
        return None;
    }

    let base = col_name.trim_end_matches("_id");
    let candidates = vec![base.to_string(), format!("{}s", base)];

    if let Ok(admin_reg) = ADMIN_REGISTRY.lock() {
        for candidate in candidates {
            if admin_reg.contains_key(candidate.as_str()) {
                return Some(candidate);
            }
        }
    }
    None
}
