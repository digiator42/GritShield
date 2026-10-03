use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// The JSON Schema dialect advertised in `inputSchema`.
///
/// MCP requires Draft 2020-12 keywords; clients key their argument UIs off
/// `properties`, `required` and `type`, all of which are stable across drafts.
pub const SCHEMA_DIALECT: &str = "https://json-schema.org/draft/2020-12/schema";

/// A single schema violation, addressed by JSON Pointer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SchemaViolation {
    /// JSON Pointer to the offending node (e.g. `/properties/title/type`).
    pub path: String,
    /// What the schema demanded.
    pub expected: String,
    /// What the payload actually held.
    pub actual: String,
}

impl SchemaViolation {
    fn new(path: impl Into<String>, expected: impl Into<String>, actual: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            expected: expected.into(),
            actual: actual.into(),
        }
    }

    pub fn describe(&self) -> String {
        if self.path.is_empty() {
            format!("{} (got {})", self.expected, self.actual)
        } else {
            format!("{} at {} (got {})", self.expected, self.path, self.actual)
        }
    }
}

/// The declarative description of one MCP tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolSchema {
    pub name: String,
    pub description: String,
    /// Valid JSON Schema describing the arguments.
    pub input_schema: Value,
    /// The GritShield role a caller must hold. `None` means unrestricted.
    pub required_role: Option<&'static str>,
    /// Whether the tool starts enabled. The kill switch can flip this at runtime.
    pub enabled: bool,
}

impl McpToolSchema {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema: normalize_schema(input_schema),
            required_role: None,
            enabled: true,
        }
    }

    pub fn requiring_role(mut self, role: &'static str) -> Self {
        self.required_role = Some(role);
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// The wire shape advertised to clients via `tools/list`.
    pub fn to_descriptor(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema,
        })
    }
}

/// Ensure a schema advertises its dialect and object type.
///
/// Handlers frequently pass a bare `{"properties": {...}}`. Prefixing
/// `$schema` and `type` here means every tool looks the same to clients without
/// each author having to remember the boilerplate.
pub fn normalize_schema(mut schema: Value) -> Value {
    if !schema.is_object() {
        schema = json!({ "type": "object", "properties": {} });
    }

    let map = schema.as_object_mut().expect("just coerced to object");
    map.entry("$schema").or_insert_with(|| json!(SCHEMA_DIALECT));
    map.entry("type").or_insert_with(|| json!("object"));
    map.entry("properties").or_insert_with(|| json!({}));
    schema
}

/// A permissive object schema, used when a tool takes no arguments.
pub fn empty_object_schema() -> Value {
    json!({ "type": "object", "properties": {} })
}

/// The result of pre-flight validation: the coerced payload plus any violations.
///
/// The coerced payload matters as much as the verdict. Models routinely send
/// `""` or `null` for an optional field, and rejecting that teaches them
/// nothing; substituting the schema's `default` instead turns a hard failure
/// into a sensible call.
#[derive(Debug, Clone)]
pub struct ValidatedArgs {
    pub value: Value,
    pub violations: Vec<SchemaViolation>,
}

impl ValidatedArgs {
    pub fn is_valid(&self) -> bool {
        self.violations.is_empty()
    }

    pub fn describe_violations(&self) -> String {
        self.violations
            .iter()
            .map(|v| v.describe())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Validate `instance` against `schema`, substituting declared defaults.
///
/// Supported keywords: `type`, `properties`, `required`, `additionalProperties`,
/// `enum`, `const`, `items`, `minimum`, `maximum`, `exclusiveMinimum`,
/// `exclusiveMaximum`, `multipleOf`, `minLength`, `maxLength`, `pattern`,
/// `minItems`, `maxItems`, `uniqueItems`, `default`, `anyOf`.
///
/// That is a deliberate subset rather than a full Draft 2020-12 implementation:
/// it covers everything an MCP tool realistically needs, and it keeps the
/// framework free of a heavyweight validation dependency. Unknown keywords are
/// ignored rather than rejected, so a richer schema degrades to "checked what
/// we understood" instead of hard-failing a legitimate call.
pub fn validate(schema: &Value, instance: &Value) -> ValidatedArgs {
    let mut value = instance.clone();
    let mut violations = Vec::new();
    check(schema, &mut value, "", &mut violations);
    ValidatedArgs { value, violations }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(n) => {
            if n.is_i64() || n.is_u64() {
                "integer"
            } else {
                "number"
            }
        }
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn matches_type(expected: &str, value: &Value) -> bool {
    let actual = type_name(value);
    match expected {
        "integer" => {
            // JSON has no distinct integer type, so 3.0 is a valid integer.
            matches!(value, Value::Number(n) if n.is_i64() || n.is_u64() || n.as_f64().is_some_and(|f| f.fract() == 0.0))
        }
        "number" => matches!(value, Value::Number(_)),
        other => other == actual,
    }
}

fn check(schema: &Value, value: &mut Value, path: &str, out: &mut Vec<SchemaViolation>) {
    // A boolean schema (`true`/`false`) is legal JSON Schema.
    match schema {
        Value::Bool(true) => return,
        Value::Bool(false) => {
            out.push(SchemaViolation::new(
                path,
                "a value to be permitted by the schema",
                "a value that is forbidden",
            ));
            return;
        }
        Value::Object(_) => {}
        _ => return,
    }

    let schema = schema.as_object().expect("matched object above");

    // ── `default` ──
    // Injected before validation so the rest of the schema sees the real value.
    if let Some(default) = schema.get("default") {
        let missing = match value {
            Value::Null => true,
            Value::String(s) => s.is_empty(),
            _ => false,
        };
        if missing {
            *value = default.clone();
        }
    }

    // ── `type` ──
    if let Some(expected) = schema.get("type") {
        let accepted: Vec<&str> = match expected {
            Value::String(s) => vec![s.as_str()],
            Value::Array(items) => items.iter().filter_map(|i| i.as_str()).collect(),
            _ => Vec::new(),
        };

        if !accepted.is_empty() && !accepted.iter().any(|t| matches_type(t, value)) {
            out.push(SchemaViolation::new(
                path,
                format!("type {}", accepted.join(" or ")),
                type_name(value),
            ));
            return; // Downstream keywords assume the type matched.
        }
    }

    // ── `const` / `enum` ──
    if let Some(expected) = schema.get("const") {
        if value != expected {
            out.push(SchemaViolation::new(
                path,
                format!("the constant {}", expected),
                render(value),
            ));
        }
    }

    if let Some(Value::Array(options)) = schema.get("enum") {
        if !options.contains(value) {
            let rendered: Vec<String> = options.iter().map(render).collect();
            out.push(SchemaViolation::new(
                path,
                format!("one of [{}]", rendered.join(", ")),
                render(value),
            ));
        }
    }

    // ── `anyOf` ──
    // Satisfied if any branch validates cleanly; otherwise every branch's
    // complaints are reported so the model can see what was tried.
    if let Some(Value::Array(branches)) = schema.get("anyOf") {
        let mut branch_errors: Vec<String> = Vec::new();
        let satisfied = branches.iter().any(|branch| {
            let mut probe = value.clone();
            let mut branch_out = Vec::new();
            check(branch, &mut probe, path, &mut branch_out);
            if branch_out.is_empty() {
                *value = probe;
                true
            } else {
                branch_errors.push(branch_out[0].describe());
                false
            }
        });

        if !satisfied {
            out.push(SchemaViolation::new(
                path,
                format!("one of the {} permitted shapes [{}]", branches.len(), branch_errors.join(" | ")),
                render(value),
            ));
        }
    }

    // ── String constraints ──
    if let Value::String(text) = value {
        if let Some(min) = schema.get("minLength").and_then(Value::as_u64) {
            if (text.chars().count() as u64) < min {
                out.push(SchemaViolation::new(
                    path,
                    format!("a string of at least {} characters", min),
                    format!("{} characters", text.chars().count()),
                ));
            }
        }
        if let Some(max) = schema.get("maxLength").and_then(Value::as_u64) {
            if (text.chars().count() as u64) > max {
                out.push(SchemaViolation::new(
                    path,
                    format!("a string of at most {} characters", max),
                    format!("{} characters", text.chars().count()),
                ));
            }
        }
        if let Some(pattern) = schema.get("pattern").and_then(Value::as_str) {
            // An unparseable pattern is skipped rather than failing the call: a
            // broken regex is an author bug, not a caller bug.
            if let Ok(re) = regex::Regex::new(pattern) {
                if !re.is_match(text) {
                    out.push(SchemaViolation::new(
                        path,
                        format!("a string matching /{}/", pattern),
                        render(value),
                    ));
                }
            }
        }
    }

    // ── Numeric constraints ──
    if let Some(number) = value.as_f64() {
        if let Some(min) = schema.get("minimum").and_then(Value::as_f64) {
            if number < min {
                out.push(SchemaViolation::new(
                    path,
                    format!("a number >= {}", min),
                    render(value),
                ));
            }
        }
        if let Some(max) = schema.get("maximum").and_then(Value::as_f64) {
            if number > max {
                out.push(SchemaViolation::new(
                    path,
                    format!("a number <= {}", max),
                    render(value),
                ));
            }
        }
        if let Some(min) = schema.get("exclusiveMinimum").and_then(Value::as_f64) {
            if number <= min {
                out.push(SchemaViolation::new(
                    path,
                    format!("a number > {}", min),
                    render(value),
                ));
            }
        }
        if let Some(max) = schema.get("exclusiveMaximum").and_then(Value::as_f64) {
            if number >= max {
                out.push(SchemaViolation::new(
                    path,
                    format!("a number < {}", max),
                    render(value),
                ));
            }
        }
        if let Some(step) = schema.get("multipleOf").and_then(Value::as_f64) {
            if step > 0.0 {
                let quotient = number / step;
                if (quotient - quotient.round()).abs() > f64::EPSILON * 8.0 {
                    out.push(SchemaViolation::new(
                        path,
                        format!("a multiple of {}", step),
                        render(value),
                    ));
                }
            }
        }
    }

    // ── Array constraints ──
    if let Value::Array(items) = value {
        if let Some(min) = schema.get("minItems").and_then(Value::as_u64) {
            if (items.len() as u64) < min {
                out.push(SchemaViolation::new(
                    path,
                    format!("an array of at least {} items", min),
                    format!("{} items", items.len()),
                ));
            }
        }
        if let Some(max) = schema.get("maxItems").and_then(Value::as_u64) {
            if (items.len() as u64) > max {
                out.push(SchemaViolation::new(
                    path,
                    format!("an array of at most {} items", max),
                    format!("{} items", items.len()),
                ));
            }
        }
        if schema.get("uniqueItems").and_then(Value::as_bool) == Some(true) {
            let mut seen: Vec<&Value> = Vec::new();
            for item in items.iter() {
                if seen.contains(&item) {
                    out.push(SchemaViolation::new(
                        path,
                        "an array with no duplicate items",
                        format!("duplicate entry {}", render(item)),
                    ));
                    break;
                }
                seen.push(item);
            }
        }
        if let Some(item_schema) = schema.get("items") {
            for (index, item) in items.iter_mut().enumerate() {
                check(item_schema, item, &format!("{}/{}", path, index), out);
            }
        }
    }

    // ── Object constraints ──
    if let Value::Object(map) = value {
        if let Some(Value::Array(required)) = schema.get("required") {
            for key in required.iter().filter_map(Value::as_str) {
                // A key explicitly mapped to `null` still counts as supplied; the
                // property schema is what decides whether that is acceptable.
                if !map.contains_key(key) {
                    out.push(SchemaViolation::new(
                        format!("{}/required", path),
                        format!("the required property '{}'", key),
                        "it was absent",
                    ));
                }
            }
        }

        let properties = schema.get("properties").and_then(Value::as_object);
        if let Some(properties) = properties {
            for (key, property_schema) in properties {
                if let Some(child) = map.get_mut(key) {
                    check(
                        property_schema,
                        child,
                        &format!("{}/properties/{}", path, key),
                        out,
                    );
                } else if let Some(default) = property_schema.get("default") {
                    // `required` is checked above, so this is purely a
                    // convenience fill for an optional parameter.
                    map.insert(key.clone(), default.clone());
                }
            }
        }

        match schema.get("additionalProperties") {
            Some(Value::Bool(false)) => {
                for key in map.keys() {
                    let known = properties.is_some_and(|p| p.contains_key(key));
                    if !known {
                        out.push(SchemaViolation::new(
                            format!("{}/{}", path, key),
                            "a declared property",
                            "an undeclared property",
                        ));
                    }
                }
            }
            Some(additional) if !additional.is_boolean() => {
                for (key, child) in map.iter_mut() {
                    let known = properties.is_some_and(|p| p.contains_key(key));
                    if !known {
                        check(additional, child, &format!("{}/{}", path, key), out);
                    }
                }
            }
            _ => {}
        }
    }
}

/// Compact, model-readable rendering of a value for error messages.
///
/// A long argument object gets truncated: a violation message is fed back into
/// a model context, and echoing 4 KB of payload back at it wastes the window
/// that the actual error needed.
fn render(value: &Value) -> String {
    let text = value.to_string();
    const LIMIT: usize = 120;
    if text.chars().count() > LIMIT {
        let head: String = text.chars().take(LIMIT).collect();
        format!("{}…", head)
    } else {
        text
    }
}

/// Convenience builder for writing schemas inline in Rust.
///
/// ```no_run
/// use gritshield::mcp::schema::SchemaBuilder;
/// use serde_json::json;
///
/// let schema = SchemaBuilder::new()
///     .string("title", "Sprint title")
///     .integer("days", "Length in days")
///     .build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct SchemaBuilder {
    properties: Map<String, Value>,
    required: Vec<String>,
}

impl SchemaBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare a property with an explicit schema fragment.
    pub fn property(mut self, name: &str, schema: Value, required: bool) -> Self {
        if required {
            self.required.push(name.to_string());
        }
        self.properties.insert(name.to_string(), schema);
        self
    }

    pub fn string(self, name: &str, description: &str) -> Self {
        self.property(
            name,
            json!({ "type": "string", "description": description }),
            true,
        )
    }

    pub fn optional_string(self, name: &str, description: &str) -> Self {
        self.property(
            name,
            json!({ "type": "string", "description": description }),
            false,
        )
    }

    pub fn integer(self, name: &str, description: &str) -> Self {
        self.property(
            name,
            json!({ "type": "integer", "description": description }),
            true,
        )
    }

    pub fn optional_integer(self, name: &str, description: &str) -> Self {
        self.property(
            name,
            json!({ "type": "integer", "description": description }),
            false,
        )
    }

    pub fn boolean(self, name: &str, description: &str) -> Self {
        self.property(
            name,
            json!({ "type": "boolean", "description": description }),
            false,
        )
    }

    pub fn array_of(self, name: &str, description: &str, item_type: &str) -> Self {
        self.property(
            name,
            json!({
                "type": "array",
                "description": description,
                "items": { "type": item_type }
            }),
            false,
        )
    }

    pub fn build(self) -> Value {
        json!({
            "type": "object",
            "properties": Value::Object(self.properties),
            "required": self.required,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_property_is_enforced() {
        let schema = json!({
            "type": "object",
            "properties": { "title": { "type": "string" } },
            "required": ["title"],
        });

        let ok = validate(&schema, &json!({ "title": "Sprint 1" }));
        assert!(ok.is_valid());

        let missing = validate(&schema, &json!({}));
        assert!(!missing.is_valid());
        assert!(missing.describe_violations().contains("title"));
    }

    #[test]
    fn wrong_type_is_reported() {
        let schema = json!({ "type": "object", "properties": { "days": { "type": "integer" } } });
        let result = validate(&schema, &json!({ "days": "soon" }));
        assert!(!result.is_valid());
        assert!(result.violations[0].expected.contains("integer"));
    }

    #[test]
    fn integral_floats_satisfy_integer() {
        let schema = json!({ "type": "object", "properties": { "n": { "type": "integer" } } });
        assert!(validate(&schema, &json!({ "n": 3.0 })).is_valid());
    }

    #[test]
    fn defaults_are_applied() {
        let schema = json!({
            "type": "object",
            "properties": {
                "page": { "type": "integer", "default": 1 },
                "limit": { "type": "integer", "default": 25 }
            }
        });

        let result = validate(&schema, &json!({ "page": null }));
        assert!(result.is_valid());
        assert_eq!(result.value["page"], json!(1));
        assert_eq!(result.value["limit"], json!(25));
    }

    #[test]
    fn additional_properties_false_rejects_extras() {
        let schema = json!({
            "type": "object",
            "properties": { "a": { "type": "string" } },
            "additionalProperties": false
        });

        let result = validate(&schema, &json!({ "a": "x", "b": "y" }));
        assert!(!result.is_valid());
    }

    #[test]
    fn bounds_and_enums_are_checked() {
        let schema = json!({
            "type": "object",
            "properties": {
                "priority": { "type": "string", "enum": ["low", "high"] },
                "count": { "type": "integer", "minimum": 1, "maximum": 10 }
            }
        });

        assert!(validate(&schema, &json!({ "priority": "urgent", "count": 50 })).violations.len() == 2);
        assert!(validate(&schema, &json!({ "priority": "low", "count": 5 })).is_valid());
    }

    #[test]
    fn array_items_are_validated_elementwise() {
        let schema = json!({
            "type": "object",
            "properties": {
                "tags": { "type": "array", "items": { "type": "string" } }
            }
        });

        let result = validate(&schema, &json!({ "tags": ["ok", 4] }));
        assert!(!result.is_valid());
        assert_eq!(result.violations[0].path, "/properties/tags/1");
    }

    #[test]
    fn unknown_keywords_are_ignored_not_fatal() {
        let schema = json!({
            "type": "object",
            "$defs": { "unused": true },
            "x-vendor-extension": "whatever",
            "properties": { "a": { "type": "string" } }
        });

        assert!(validate(&schema, &json!({ "a": "fine" })).is_valid());
    }

    #[test]
    fn pattern_constraint_uses_the_regex_crate() {
        let schema = json!({
            "type": "object",
            "properties": { "slug": { "type": "string", "pattern": "^[a-z0-9-]+$" } }
        });

        assert!(validate(&schema, &json!({ "slug": "my-sprint" })).is_valid());
        assert!(!validate(&schema, &json!({ "slug": "My Sprint!" })).is_valid());
    }

    #[test]
    fn normalize_fills_dialect_and_object_type() {
        let normalized = normalize_schema(json!({ "properties": { "a": { "type": "string" } } }));
        assert_eq!(normalized["$schema"], json!(SCHEMA_DIALECT));
        assert_eq!(normalized["type"], json!("object"));
    }

    #[test]
    fn builder_marks_required_fields() {
        let schema = SchemaBuilder::new().string("a", "first").optional_integer("b", "second").build();
        assert_eq!(schema["required"], json!(["a"]));
        assert_eq!(schema["properties"]["b"]["type"], json!("integer"));
    }
}