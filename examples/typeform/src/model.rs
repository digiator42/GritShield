use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FieldKind {
    #[default]
    Text,
    Number,
    Email,
    Select,
}

/// A dynamic rule: this field's constraints apply only when
/// `answers[field] == equals`. e.g. "company size is required only if
/// sector equals 'enterprise'".
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub field: String,
    pub equals: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDef {
    pub name: String,
    pub label: String,
    #[serde(rename = "type", default)]
    pub kind: FieldKind,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub condition: Option<Condition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Step {
    #[serde(default)]
    pub title: String,
    pub fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormDef {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub steps: Vec<Step>,
}

impl FormDef {
    pub fn field_count(&self) -> usize {
        self.steps.iter().map(|s| s.fields.len()).sum()
    }
}

/// Coarse structural validation of a submitted form definition.
pub fn validate_definition(def: &FormDef) -> Result<(), String> {
    if def.slug.is_empty() {
        return Err("slug is required".into());
    }
    if def.slug.len() > 64
        || !def
            .slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err("slug must be 1-64 chars of lowercase letters, digits and '-'".into());
    }
    if def.title.trim().is_empty() {
        return Err("title is required".into());
    }
    if def.steps.is_empty() {
        return Err("at least one step is required".into());
    }
    let mut seen: Vec<&str> = Vec::new();
    for step in &def.steps {
        for field in &step.fields {
            if field.name.trim().is_empty() {
                return Err("every field needs a name".into());
            }
            if !field.name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Err(format!("invalid field name `{}`", field.name));
            }
            if seen.contains(&field.name.as_str()) {
                return Err(format!("duplicate field name `{}`", field.name));
            }
            seen.push(&field.name);
            if matches!(field.kind, FieldKind::Select) && field.options.is_empty() {
                return Err(format!("select field `{}` needs at least one option", field.name));
            }
        }
    }
    Ok(())
}