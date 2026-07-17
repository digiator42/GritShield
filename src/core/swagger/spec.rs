use super::models::*;
use crate::core::schema::SCHEMA_REGISTRY;
use crate::database::repository::registry::{ACTIONS_REGISTRY, ADMIN_REGISTRY};
use crate::http::request::HttpMethod;
use crate::routing::trie::AutoRoute;
use std::collections::HashMap;

pub fn generate_openapi_spec() -> OpenApiSpec {
    let registry = ADMIN_REGISTRY.lock().unwrap();
    let mut paths = HashMap::new();
    let mut schemas = HashMap::new();
    let mut tags = Vec::new();

    // ============================================================
    // DEVELOPER ROUTES (from AutoRoute inventory)
    // ============================================================
    let dev_tag = Tag {
        name: "Developer Routes".to_string(),
        description: "Routes defined by the developer".to_string(),
    };
    tags.push(dev_tag);

    for route in inventory::iter::<AutoRoute> {
        let path = route.path;

        // Fully sanitize and construct compliant openapi curly paths
        let mut openapi_path = path.to_string();
        let mut parameters = Vec::new();

        for segment in path.split('/') {
            if segment.starts_with(':') {
                let param_name = segment
                    .trim_start_matches(':')
                    .trim_matches(|c| c == '{' || c == '}' || c == ':')
                    .to_string();

                if !param_name.is_empty() {
                    openapi_path = openapi_path
                        .replace(&format!(":{}", param_name), &format!("{{{}}}", param_name));

                    parameters.push(Parameter {
                        name: param_name.clone(),
                        in_: "path".to_string(),
                        required: true,
                        schema: Schema {
                            type_: "string".to_string(),
                            format: None,
                            properties: None,
                            required: None,
                            items: None,
                            enum_values: None,
                        },
                        description: Some(format!("{} parameter", param_name)),
                    });
                }
            }
        }

        let method = route.method;
        let method_str = format!("{:?}", method).to_lowercase();

        if path.contains("page") {
            parameters.push(Parameter {
                name: "page".to_string(),
                in_: "query".to_string(),
                required: false,
                schema: Schema {
                    type_: "integer".to_string(),
                    format: Some("int32".to_string()),
                    properties: None,
                    required: None,
                    items: None,
                    enum_values: None,
                },
                description: Some("Page number".to_string()),
            });
        }

        // Build request body from schema registry if present
        let request_body = if let Some(schema_name) = route.request_body_schema {
            let registry = SCHEMA_REGISTRY.lock().unwrap();

            // matching fallback (checks exact match, then snake_case/lowercase equivalents)
            let search_name = schema_name.to_string();
            let search_lower = search_name.to_lowercase();
            let search_snake = search_name.replace(" ", "_").to_lowercase();

            let model_schema_opt = registry
                .get(&search_name)
                .or_else(|| registry.get(&search_lower))
                .or_else(|| {
                    registry
                        .iter()
                        .find(|(k, _)| {
                            k.to_lowercase() == search_lower || k.to_lowercase() == search_snake
                        })
                        .map(|(_, v)| v)
                });

            if let Some(model_schema) = model_schema_opt {
                let mut properties = HashMap::new();
                let mut required = Vec::new();

                for field in &model_schema.fields {
                    let type_ = match field.type_.as_str() {
                        "i64" | "i32" | "i16" | "u64" | "u32" | "u16" => Schema {
                            type_: "integer".to_string(),
                            format: Some("int64".to_string()),
                            properties: None,
                            required: None,
                            items: None,
                            enum_values: None,
                        },
                        "NaiveDateTime" | "DateTime" => Schema {
                            type_: "string".to_string(),
                            format: Some("date-time".to_string()),
                            properties: None,
                            required: None,
                            items: None,
                            enum_values: None,
                        },
                        "bool" => Schema {
                            type_: "boolean".to_string(),
                            format: None,
                            properties: None,
                            required: None,
                            items: None,
                            enum_values: None,
                        },
                        _ => Schema {
                            type_: "string".to_string(),
                            format: None,
                            properties: None,
                            required: None,
                            items: None,
                            enum_values: None,
                        },
                    };
                    properties.insert(field.name.clone(), type_);
                    if !field.nullable {
                        required.push(field.name.clone());
                    }
                }

                Some(RequestBody {
                    required: true,
                    content: {
                        let mut content = HashMap::new();
                        content.insert(
                            "application/json".to_string(),
                            MediaType {
                                schema: Schema {
                                    type_: "object".to_string(),
                                    format: None,
                                    properties: Some(properties),
                                    required: Some(required),
                                    items: None,
                                    enum_values: None,
                                },
                            },
                        );
                        content
                    },
                })
            } else {
                None
            }
        } else {
            None
        };

        let operation = Operation {
            summary: format!("{} {}", method_str.to_uppercase(), path),
            description: Some(format!("route: {}", path)),
            operation_id: format!(
                "dev_{}_{}",
                method_str,
                path.replace("/", "_")
                    .replace(":", "_")
                    .replace("{", "_")
                    .replace("}", "_")
            ),
            tags: vec!["Developer Routes".to_string()],
            parameters,
            responses: {
                let mut res = HashMap::new();
                res.insert(
                    "200".to_string(),
                    Response {
                        description: "Success".to_string(),
                        content: Some({
                            let mut content = HashMap::new();
                            content.insert(
                                "application/json".to_string(), // Formatted to JSON return type
                                MediaType {
                                    schema: Schema {
                                        type_: "object".to_string(),
                                        format: None,
                                        properties: None,
                                        required: None,
                                        items: None,
                                        enum_values: None,
                                    },
                                },
                            );
                            content
                        }),
                    },
                );
                res.insert(
                    "404".to_string(),
                    Response {
                        description: "Not Found".to_string(),
                        content: None,
                    },
                );
                res
            },
            request_body,
        };

        let path_item = paths.entry(openapi_path).or_insert(PathItem {
            get: None,
            post: None,
            put: None,
            delete: None,
            patch: None,
        });

        match method {
            HttpMethod::GET => path_item.get = Some(operation),
            HttpMethod::POST => path_item.post = Some(operation),
            HttpMethod::PUT => path_item.put = Some(operation),
            HttpMethod::DELETE => path_item.delete = Some(operation),
            HttpMethod::PATCH => path_item.patch = Some(operation),
            _ => {}
        }
    }

    // ============================================================
    // ADMIN ROUTES (from ADMIN_REGISTRY)
    // ============================================================
    for (table_slug, meta) in registry.iter() {
        tags.push(Tag {
            name: table_slug.to_string(),
            description: format!("{} management endpoints", table_slug),
        });

        let mut properties = HashMap::new();
        let mut required = Vec::new();

        for col in &meta.searchable_columns {
            let col_str = col.as_ref();
            let type_ = match col_str {
                "id" => Schema {
                    type_: "integer".to_string(),
                    format: Some("int64".to_string()),
                    properties: None,
                    required: None,
                    items: None,
                    enum_values: None,
                },
                "created_at" | "updated_at" | "timestamp" => Schema {
                    type_: "string".to_string(),
                    format: Some("date-time".to_string()),
                    properties: None,
                    required: None,
                    items: None,
                    enum_values: None,
                },
                _ => Schema {
                    type_: "string".to_string(),
                    format: None,
                    properties: None,
                    required: None,
                    items: None,
                    enum_values: None,
                },
            };
            let col_name = col_str.to_string();
            properties.insert(col_name.clone(), type_);
            required.push(col_name);
        }

        if !properties.contains_key("id") {
            properties.insert(
                "id".to_string(),
                Schema {
                    type_: "integer".to_string(),
                    format: Some("int64".to_string()),
                    properties: None,
                    required: None,
                    items: None,
                    enum_values: None,
                },
            );
            required.insert(0, "id".to_string());
        }

        let schema_name = format!("{}Model", table_slug);
        schemas.insert(
            schema_name,
            Schema {
                type_: "object".to_string(),
                format: None,
                properties: Some(properties),
                required: Some(required),
                items: None,
                enum_values: None,
            },
        );

        // ---- LIST ENDPOINT ----
        let mut list_params = vec![
            Parameter {
                name: "page".to_string(),
                in_: "query".to_string(),
                required: false,
                schema: Schema {
                    type_: "integer".to_string(),
                    format: Some("int32".to_string()),
                    properties: None,
                    required: None,
                    items: None,
                    enum_values: None,
                },
                description: Some("Page number".to_string()),
            },
            Parameter {
                name: "q".to_string(),
                in_: "query".to_string(),
                required: false,
                schema: Schema {
                    type_: "string".to_string(),
                    format: None,
                    properties: None,
                    required: None,
                    items: None,
                    enum_values: None,
                },
                description: Some("Search query".to_string()),
            },
        ];

        for col in &meta.searchable_columns {
            list_params.push(Parameter {
                name: format!("filter__{}__op", col),
                in_: "query".to_string(),
                required: false,
                schema: Schema {
                    type_: "string".to_string(),
                    format: None,
                    properties: None,
                    required: None,
                    items: None,
                    enum_values: Some(vec![
                        "eq".to_string(),
                        "ne".to_string(),
                        "gt".to_string(),
                        "gte".to_string(),
                        "lt".to_string(),
                        "lte".to_string(),
                        "contains".to_string(),
                        "startswith".to_string(),
                        "endswith".to_string(),
                        "is_null".to_string(),
                        "is_not_null".to_string(),
                    ]),
                },
                description: Some(format!("Filter operator for column '{}'", col)),
            });
            list_params.push(Parameter {
                name: format!("filter__{}__value", col),
                in_: "query".to_string(),
                required: false,
                schema: Schema {
                    type_: "string".to_string(),
                    format: None,
                    properties: None,
                    required: None,
                    items: None,
                    enum_values: None,
                },
                description: Some(format!("Filter value for column '{}'", col)),
            });
        }

        let base_route = meta.route_path.to_string();
        let list_item = paths.entry(base_route.clone()).or_insert(PathItem {
            get: None,
            post: None,
            put: None,
            delete: None,
            patch: None,
        });
        list_item.get = Some(Operation {
            summary: format!("List {} records", table_slug),
            description: Some(format!("Get a paginated list of {} records.", table_slug)),
            operation_id: format!("list_{}", table_slug),
            tags: vec![table_slug.to_string()],
            parameters: list_params,
            responses: {
                let mut res = HashMap::new();
                res.insert(
                    "200".to_string(),
                    Response {
                        description: "Success".to_string(),
                        content: Some({
                            let mut content = HashMap::new();
                            content.insert(
                                "text/html".to_string(),
                                MediaType {
                                    schema: Schema {
                                        type_: "string".to_string(),
                                        format: None,
                                        properties: None,
                                        required: None,
                                        items: None,
                                        enum_values: None,
                                    },
                                },
                            );
                            content
                        }),
                    },
                );
                res
            },
            request_body: None,
        });

        // ---- DETAIL ENDPOINT ----
        let detail_path = format!("{}/{{id}}", base_route.trim_end_matches('/'));
        let detail_item = paths.entry(detail_path).or_insert(PathItem {
            get: None,
            post: None,
            put: None,
            delete: None,
            patch: None,
        });
        detail_item.get = Some(Operation {
            summary: format!("Get {} record details", table_slug),
            description: Some(format!(
                "Get a single {} record with all fields.",
                table_slug
            )),
            operation_id: format!("get_{}", table_slug),
            tags: vec![table_slug.to_string()],
            parameters: vec![Parameter {
                name: "id".to_string(),
                in_: "path".to_string(),
                required: true,
                schema: Schema {
                    type_: "string".to_string(),
                    format: None,
                    properties: None,
                    required: None,
                    items: None,
                    enum_values: None,
                },
                description: Some("Record ID".to_string()),
            }],
            responses: {
                let mut res = HashMap::new();
                res.insert(
                    "200".to_string(),
                    Response {
                        description: "Success".to_string(),
                        content: Some({
                            let mut content = HashMap::new();
                            content.insert(
                                "text/html".to_string(),
                                MediaType {
                                    schema: Schema {
                                        type_: "string".to_string(),
                                        format: None,
                                        properties: None,
                                        required: None,
                                        items: None,
                                        enum_values: None,
                                    },
                                },
                            );
                            content
                        }),
                    },
                );
                res.insert(
                    "404".to_string(),
                    Response {
                        description: "Not Found".to_string(),
                        content: None,
                    },
                );
                res
            },
            request_body: None,
        });

        // Single-record DELETE operation
        detail_item.delete = Some(Operation {
            summary: format!("Delete {} record", table_slug),
            description: Some(format!("Delete a single {} record by its ID.", table_slug)),
            operation_id: format!("delete_{}", table_slug),
            tags: vec![table_slug.to_string()],
            parameters: vec![Parameter {
                name: "id".to_string(),
                in_: "path".to_string(),
                required: true,
                schema: Schema {
                    type_: "string".to_string(),
                    format: None,
                    properties: None,
                    required: None,
                    items: None,
                    enum_values: None,
                },
                description: Some("Record ID to delete".to_string()),
            }],
            responses: {
                let mut res = HashMap::new();
                res.insert(
                    "200".to_string(),
                    Response {
                        description: "Record successfully deleted".to_string(),
                        content: None,
                    },
                );
                res.insert(
                    "404".to_string(),
                    Response {
                        description: "Record not found".to_string(),
                        content: None,
                    },
                );
                res
            },
            request_body: None,
        });

        // ---- BULK DELETE ----
        let bulk_path = format!("{}/bulk-delete", base_route.trim_end_matches('/'));
        let bulk_item = paths.entry(bulk_path).or_insert(PathItem {
            get: None,
            post: None,
            put: None,
            delete: None,
            patch: None,
        });
        bulk_item.post = Some(Operation {
            summary: format!("Bulk delete {} records", table_slug),
            description: Some(format!("Delete multiple {} records by ID.", table_slug)),
            operation_id: format!("bulk_delete_{}", table_slug),
            tags: vec![table_slug.to_string()],
            parameters: vec![],
            responses: {
                let mut res = HashMap::new();
                res.insert(
                    "200".to_string(),
                    Response {
                        description: "Success".to_string(),
                        content: None,
                    },
                );
                res
            },
            request_body: Some(RequestBody {
                required: true,
                content: {
                    let mut content = HashMap::new();
                    content.insert(
                        "application/x-www-form-urlencoded".to_string(),
                        MediaType {
                            schema: Schema {
                                type_: "object".to_string(),
                                format: None,
                                properties: Some({
                                    let mut props = HashMap::new();
                                    props.insert(
                                        "ids".to_string(),
                                        Schema {
                                            type_: "string".to_string(),
                                            format: None,
                                            properties: None,
                                            required: None,
                                            items: None,
                                            enum_values: None,
                                        },
                                    );
                                    props
                                }),
                                required: Some(vec!["ids".to_string()]),
                                items: None,
                                enum_values: None,
                            },
                        },
                    );
                    content
                },
            }),
        });

        // ---- PATCH CELL ----
        let patch_path = format!("{}/update-cell", base_route.trim_end_matches('/'));
        let patch_item = paths.entry(patch_path).or_insert(PathItem {
            get: None,
            post: None,
            put: None,
            delete: None,
            patch: None,
        });
        patch_item.patch = Some(Operation {
            summary: format!("Update a single cell in {} table", table_slug),
            description: Some(format!(
                "Update a single field/column of a {} record.",
                table_slug
            )),
            operation_id: format!("patch_cell_{}", table_slug),
            tags: vec![table_slug.to_string()],
            parameters: vec![],
            responses: {
                let mut res = HashMap::new();
                res.insert(
                    "200".to_string(),
                    Response {
                        description: "Success".to_string(),
                        content: Some({
                            let mut content = HashMap::new();
                            content.insert(
                                "text/html".to_string(),
                                MediaType {
                                    schema: Schema {
                                        type_: "string".to_string(),
                                        format: None,
                                        properties: None,
                                        required: None,
                                        items: None,
                                        enum_values: None,
                                    },
                                },
                            );
                            content
                        }),
                    },
                );
                res.insert(
                    "400".to_string(),
                    Response {
                        description: "Bad Request".to_string(),
                        content: None,
                    },
                );
                res
            },
            request_body: Some(RequestBody {
                required: true,
                content: {
                    let mut content = HashMap::new();
                    content.insert(
                        "application/x-www-form-urlencoded".to_string(),
                        MediaType {
                            schema: Schema {
                                type_: "object".to_string(),
                                format: None,
                                properties: Some({
                                    let mut props = HashMap::new();
                                    props.insert(
                                        "id".to_string(),
                                        Schema {
                                            type_: "string".to_string(),
                                            format: None,
                                            properties: None,
                                            required: None,
                                            items: None,
                                            enum_values: None,
                                        },
                                    );
                                    props.insert(
                                        "column".to_string(),
                                        Schema {
                                            type_: "string".to_string(),
                                            format: None,
                                            properties: None,
                                            required: None,
                                            items: None,
                                            enum_values: None,
                                        },
                                    );
                                    props.insert(
                                        "table_to_modify".to_string(),
                                        Schema {
                                            type_: "string".to_string(),
                                            format: None,
                                            properties: None,
                                            required: None,
                                            items: None,
                                            enum_values: None,
                                        },
                                    );
                                    props
                                }),
                                required: Some(vec!["id".to_string(), "column".to_string()]),
                                items: None,
                                enum_values: None,
                            },
                        },
                    );
                    content
                },
            }),
        });
    }

    // ============================================================
    // CUSTOM ACTION ENDPOINTS
    // ============================================================
    tags.push(Tag {
        name: "custom_actions".to_string(),
        description: "Custom developer-defined actions".to_string(),
    });

    let actions_registry = ACTIONS_REGISTRY.lock().unwrap();
    for (table_slug, actions) in actions_registry.iter() {
        for action in actions {
            let action_path = format!("/admin/{}/action/{{action_name}}", table_slug);
            let action_item = paths.entry(action_path).or_insert(PathItem {
                get: None,
                post: None,
                put: None,
                delete: None,
                patch: None,
            });
            action_item.post = Some(Operation {
                summary: format!(
                    "Execute custom action '{}' on {} table",
                    action.label, table_slug
                ),
                description: Some(format!(
                    "Execute the '{}' custom action for {} records.",
                    action.label, table_slug
                )),
                operation_id: format!("action_{}_{}", table_slug, action.label.replace(" ", "_")),
                tags: vec![table_slug.to_string(), "custom_actions".to_string()],
                parameters: vec![Parameter {
                    name: "action_name".to_string(),
                    in_: "path".to_string(),
                    required: true,
                    schema: Schema {
                        type_: "string".to_string(),
                        format: None,
                        properties: None,
                        required: None,
                        items: None,
                        enum_values: Some(vec![action.label.to_string()]),
                    },
                    description: Some("Action name".to_string()),
                }],
                responses: {
                    let mut res = HashMap::new();
                    res.insert(
                        "200".to_string(),
                        Response {
                            description: "Success".to_string(),
                            content: Some({
                                let mut content = HashMap::new();
                                content.insert(
                                    "text/html".to_string(),
                                    MediaType {
                                        schema: Schema {
                                            type_: "string".to_string(),
                                            format: None,
                                            properties: None,
                                            required: None,
                                            items: None,
                                            enum_values: None,
                                        },
                                    },
                                );
                                content
                            }),
                        },
                    );
                    res
                },
                request_body: Some(RequestBody {
                    required: true,
                    content: {
                        let mut content = HashMap::new();
                        content.insert(
                            "application/x-www-form-urlencoded".to_string(),
                            MediaType {
                                schema: Schema {
                                    type_: "object".to_string(),
                                    format: None,
                                    properties: Some({
                                        let mut props = HashMap::new();
                                        props.insert(
                                            "ids".to_string(),
                                            Schema {
                                                type_: "array".to_string(),
                                                format: None,
                                                properties: None,
                                                required: None,
                                                items: Some(Box::new(Schema {
                                                    type_: "string".to_string(),
                                                    format: None,
                                                    properties: None,
                                                    required: None,
                                                    items: None,
                                                    enum_values: None,
                                                })),
                                                enum_values: None,
                                            },
                                        );
                                        props
                                    }),
                                    required: Some(vec!["ids".to_string()]),
                                    items: None,
                                    enum_values: None,
                                },
                            },
                        );
                        content
                    },
                }),
            });
        }
    }

    OpenApiSpec {
        openapi: "3.0.0".to_string(),
        info: Info {
            title: "OpenApi API".to_string(),
            version: "1.0.0".to_string(),
            description: "Auto-generated API documentation".to_string(),
        },
        paths,
        components: Components { schemas },
        tags,
    }
}
