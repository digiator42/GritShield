use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use gritshield::prelude::OnceLazy;

use crate::model::FormDef;

#[derive(Default)]
pub struct Store {
    pub forms: HashMap<String, FormDef>,
    /// Completed submissions per form slug.
    pub responses: HashMap<String, usize>,
    pub total_completed: usize,
    pub total_requests: usize,
}

static STORE: OnceLazy<Arc<Mutex<Store>>> = OnceLazy::new(|| Arc::new(Mutex::new(Store::default())));

pub fn store() -> Arc<Mutex<Store>> {
    STORE.clone()
}

pub fn create_form(def: FormDef) {
    let shared = store();
    let mut store = shared.lock().unwrap();
    store.forms.insert(def.slug.clone(), def);
}

pub fn get_form(slug: &str) -> Option<FormDef> {
    store().lock().unwrap().forms.get(slug).cloned()
}

pub fn list_forms() -> Vec<(String, FormDef, usize)> {
    let shared = store();
    let store = shared.lock().unwrap();
    let mut out: Vec<(String, FormDef, usize)> = store
        .forms
        .values()
        .map(|f| {
            (
                f.slug.clone(),
                f.clone(),
                store.responses.get(&f.slug).copied().unwrap_or(0),
            )
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}