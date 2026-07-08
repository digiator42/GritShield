// src/gritadmin/swagger.rs
use crate::database::repository::{ACTIONS_REGISTRY, ADMIN_REGISTRY};
use crate::prelude::*;
use crate::protocol::request::HttpMethod;
use crate::routing::trie::AutoRoute;
use serde_json::json;
use std::collections::HashMap;

#[derive(Debug, Clone, serde::Serialize)]
pub struct OpenApiSpec {
    pub openapi: String,
    pub info: Info,
    pub paths: HashMap<String, PathItem>,
    pub components: Components,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Info {
    pub title: String,
    pub version: String,
    pub description: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PathItem {
    pub get: Option<Operation>,
    pub post: Option<Operation>,
    pub put: Option<Operation>,
    pub delete: Option<Operation>,
    pub patch: Option<Operation>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Operation {
    pub summary: String,
    pub description: Option<String>,
    pub operation_id: String,
    pub tags: Vec<String>,
    pub parameters: Vec<Parameter>,
    pub responses: HashMap<String, Response>,
    pub request_body: Option<RequestBody>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Parameter {
    pub name: String,
    pub in_: String,
    pub required: bool,
    pub schema: Schema,
    pub description: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RequestBody {
    pub required: bool,
    pub content: HashMap<String, MediaType>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MediaType {
    pub schema: Schema,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Response {
    pub description: String,
    pub content: Option<HashMap<String, MediaType>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Schema {
    #[serde(rename = "type")]
    pub type_: String,
    pub format: Option<String>,
    pub properties: Option<HashMap<String, Schema>>,
    pub required: Option<Vec<String>>,
    pub items: Option<Box<Schema>>,
    pub enum_values: Option<Vec<String>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Components {
    pub schemas: HashMap<String, Schema>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Tag {
    pub name: String,
    pub description: String,
}

/// Generate the OpenAPI specification from registered schemas
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
        let method = route.method;
        let method_str = format!("{:?}", method).to_lowercase();

        // Build parameters from path (extract :param placeholders)
        let mut parameters = Vec::new();
        for segment in path.split('/') {
            if segment.starts_with(':') {
                let param_name = segment.trim_start_matches(':').to_string();
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

        // Add query parameters if present (we can detect from path)
        let has_page = path.contains("page");
        if has_page {
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

        let operation = Operation {
            summary: format!("{} {}", method_str.to_uppercase(), path),
            description: Some(format!("Developer-defined route: {}", path)),
            operation_id: format!(
                "dev_{}_{}",
                method_str,
                path.replace("/", "_").replace(":", "_")
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
        };

        // Insert the operation into the paths map
        let path_item = paths.entry(path.to_string()).or_insert(PathItem {
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
        // Add tags for each table
        tags.push(Tag {
            name: table_slug.to_string(),
            description: format!("{} management endpoints", table_slug),
        });

        // Build schemas for each table
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

        paths.insert(
            meta.route_path.to_string(),
            PathItem {
                get: Some(Operation {
                    summary: format!("List {} records", table_slug),
                    description: Some(format!(
                        "Get a paginated list of {} records with filtering, sorting, and search.",
                        table_slug
                    )),
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
                }),
                post: None,
                put: None,
                delete: None,
                patch: None,
            },
        );

        // ---- DETAIL ENDPOINT ----
        let detail_path = format!("{}/:id", meta.route_path);
        paths.insert(
            detail_path,
            PathItem {
                get: Some(Operation {
                    summary: format!("Get {} record details", table_slug),
                    description: Some(format!(
                        "Get a single {} record with all fields and audit history.",
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
                }),
                post: None,
                put: None,
                delete: None,
                patch: None,
            },
        );

        // ---- BULK DELETE ----
        let bulk_path = format!("{}/bulk-delete", meta.route_path);
        paths.insert(
            bulk_path,
            PathItem {
                get: None,
                post: Some(Operation {
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
                }),
                put: None,
                delete: None,
                patch: None,
            },
        );

        // ---- PATCH CELL ----
        let patch_path = format!("{}/update-cell", meta.route_path);
        paths.insert(
            patch_path,
            PathItem {
                get: None,
                post: None,
                put: None,
                delete: None,
                patch: Some(Operation {
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
                                        required: Some(vec![
                                            "id".to_string(),
                                            "column".to_string(),
                                        ]),
                                        items: None,
                                        enum_values: None,
                                    },
                                },
                            );
                            content
                        },
                    }),
                }),
            },
        );
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
            paths.insert(
                action_path,
                PathItem {
                    get: None,
                    post: Some(Operation {
                        summary: format!(
                            "Execute custom action '{}' on {} table",
                            action.label, table_slug
                        ),
                        description: Some(format!(
                            "Execute the '{}' custom action for {} records.",
                            action.label, table_slug
                        )),
                        operation_id: format!("action_{}_{}", table_slug, action.label),
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
                                enum_values: Some(vec![action.label.to_string().clone()]),
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
                    }),
                    put: None,
                    delete: None,
                    patch: None,
                },
            );
        }
    }

    // Build final spec
    OpenApiSpec {
        openapi: "3.0.0".to_string(),
        info: Info {
            title: "GritAdmin API".to_string(),
            version: "1.0.0".to_string(),
            description: "Auto-generated API documentation from GritAdmin".to_string(),
        },
        paths,
        components: Components { schemas },
        tags,
    }
}

/// Generate the HTML page with Swagger UI
pub fn render_swagger_ui() -> Markup {
    let spec_json = serde_json::to_string_pretty(&generate_openapi_spec()).unwrap();

    html! {
        (maud::DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { "GritAdmin API Documentation - Swagger UI" }
                link rel="stylesheet" type="text/css" href="https://unpkg.com/swagger-ui-dist@5.17.14/swagger-ui.css";
                link rel="icon" type="image/png" href="https://unpkg.com/swagger-ui-dist@5.17.14/favicon-32x32.png" sizes="32x32";
                link rel="icon" type="image/png" href="https://unpkg.com/swagger-ui-dist@5.17.14/favicon-16x16.png" sizes="16x16";
                style {
                    "
                    html {
                        box-sizing: border-box;
                        overflow: -moz-scrollbars-vertical;
                        overflow-y: scroll;
                    }
                    *, *:before, *:after {
                        box-sizing: inherit;
                    }
                    body {
                        margin: 0;
                        background: #0b0b10;
                        font-family: sans-serif;
                    }
                    #swagger-ui {
                        background: #0b0b10;
                        min-height: 100vh;
                        max-width: 80%;
                        margin: auto;
                    }
                    .swagger-ui .info .title p {
                        color: #ffffff;
                    }
                    .swagger-ui .info .title small {
                        color: #64ffda;
                    }
                    .swagger-ui .info .description {
                        color: #ebeaea;
                    }
                    .swagger-ui .scheme-container {
                        background: #16213e;
                    }
                    .swagger-ui .opblock .opblock-summary-method {
                        min-width: 80px;
                    }
                    .swagger-ui .btn {
                        border-color: #64ffda;
                        color: #64ffda;
                    }
                    .swagger-ui .btn:hover {
                        background: rgba(100, 255, 218, 0.1);
                    }

                    /* Fix dimmed inline method text */
                    .swagger-ui .opblock .opblock-summary-path {
                        color: #ffffff !important;
                    }
                    .swagger-ui .opblock .opblock-summary-path a {
                        color: #ffffff !important;
                    }
                    .swagger-ui .opblock .opblock-summary-path span {
                        color: #ffffff !important;
                    }
                    .swagger-ui .opblock .opblock-summary-description {
                        color: #cccccc !important;
                    }
                    "
                }
            }
            body {
                div id="swagger-ui" {}
                script src="https://unpkg.com/swagger-ui-dist@5.17.14/swagger-ui-bundle.js" {}
                script src="https://unpkg.com/swagger-ui-dist@5.17.14/swagger-ui-standalone-preset.js" {}
                script {
                    (maud::PreEscaped(format!(r#"
                        window.onload = function() {{
                            const ui = SwaggerUIBundle({{
                                dom_id: '#swagger-ui',
                                spec: {},
                                presets: [
                                    SwaggerUIBundle.presets.apis,
                                    SwaggerUIStandalonePreset
                                ],
                                layout: "BaseLayout",
                                deepLinking: true,
                                showExtensions: true,
                                showCommonExtensions: true,
                                docExpansion: "list",
                                filter: true,
                                persistAuthorization: true,
                                defaultModelsExpandDepth: 1,
                                defaultModelExpandDepth: 1,
                                tryItOutEnabled: true,
                            }});
                            window.ui = ui;
                        }};
                    "#, spec_json)))
                }
            }
        }
    }
}
