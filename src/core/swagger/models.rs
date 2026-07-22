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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub get: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub put: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete: Option<Operation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<Operation>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Operation {
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "operationId")] // OpenAPI requires camelCase
    pub operation_id: String,
    pub tags: Vec<String>,
    pub parameters: Vec<Parameter>,
    pub responses: HashMap<String, SwaggerResponse>,
    #[serde(rename = "requestBody", skip_serializing_if = "Option::is_none")] // OpenAPI requires camelCase
    pub request_body: Option<RequestBody>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "in")] // OpenAPI requires keyword "in"
    pub in_: String,
    pub required: bool,
    pub schema: Schema,
    #[serde(skip_serializing_if = "Option::is_none")]
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
pub struct SwaggerResponse {
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<HashMap<String, MediaType>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Schema {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<HashMap<String, Schema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<Schema>>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
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