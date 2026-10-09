use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::model::{Condition, FieldDef, FieldKind, FormDef};

/// Evaluate one step's answers against its fields, honoring every conditional
/// rule. Returns a map of field name -> list of human messages.
pub fn validate_step(
    def: &FormDef,
    step_index: usize,
    answers: &Map<String, Value>,
) -> HashMap<String, Vec<String>> {
    let mut errors: HashMap<String, Vec<String>> = HashMap::new();
    let Some(step) = def.steps.get(step_index) else {
        return errors;
    };
    for field in &step.fields {
        validate_field(field, answers, &mut errors);
    }
    errors
}

fn as_string(answers: &Map<String, Value>, name: &str) -> Option<String> {
    match answers.get(name) {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        _ => None,
    }
}

fn push(errors: &mut HashMap<String, Vec<String>>, field: &str, msg: impl Into<String>) {
    errors.entry(field.to_string()).or_default().push(msg.into());
}

/// A conditional rule: `condition_met(field.condition, answers) == false`
/// means the field is neither required nor type-checked.
pub fn condition_met(cond: &Condition, answers: &Map<String, Value>) -> bool {
    matches!(as_string(answers, &cond.field), Some(v) if v == cond.equals)
}

fn is_required(field: &FieldDef, answers: &Map<String, Value>) -> bool {
    if let Some(cond) = &field.condition {
        if !condition_met(cond, answers) {
            return false;
        }
    }
    field.required
}

fn validate_field(
    field: &FieldDef,
    answers: &Map<String, Value>,
    errors: &mut HashMap<String, Vec<String>>,
) {
    let value = as_string(answers, &field.name);
    let filled = value.as_deref().map(|v| !v.trim().is_empty()).unwrap_or(false);

    // The showcase rule: "Field B required only if Field A equals 'Yes'".
    if !is_required(field, answers) && !filled {
        return;
    }

    if is_required(field, answers) && !filled {
        push(errors, &field.name, "This field is required.");
        return;
    }

    let Some(value) = value else { return };
    let value = value.trim();

    match field.kind {
        FieldKind::Text => {}
        FieldKind::Number => {
            if value.parse::<f64>().is_err() {
                push(errors, &field.name, "Must be a number.");
            }
        }
        FieldKind::Email => {
            let looks_like_email = value.contains('@') && value.split('@').count() == 2 && value.contains('.');
            if !looks_like_email {
                push(errors, &field.name, "Must be a valid email address.");
            }
        }
        FieldKind::Select => {
            if !field.options.is_empty() && !field.options.iter().any(|o| o == value) {
                push(errors, &field.name, "Must be one of: {options}.");
            }
        }
    }
}