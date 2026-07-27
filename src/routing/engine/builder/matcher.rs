use crate::http::request::HttpMethod;
use crate::routing::engine::{Router, RoutingResult};
use crate::security::xss::UntrustedString;
use std::collections::HashMap;

impl Router {
    pub fn match_route<'a>(&'a self, method: &HttpMethod, path: &str) -> RoutingResult<'a> {
    let mut current = &self.root;
    let mut captured_values = Vec::new();

    let trimmed_path = path.trim_matches('/');
    let segments: Vec<&str> = if trimmed_path.is_empty() {
        Vec::new()
    } else {
        trimmed_path.split('/').collect()
    };

    for segment in segments {
        if let Some(next_node) = current.children.get(segment) {
            current = next_node;
        } else {
            let param_match = current
                .children
                .iter()
                .find(|(key, _)| key.starts_with(':'));

            if let Some((_, param_node)) = param_match {
                // Collect the actual path parameter value (e.g. "2", "1")
                captured_values.push(segment.to_string());
                current = param_node;
            } else {
                return RoutingResult::NotFound;
            }
        }
    }

    match current.methods.get(method) {
        Some(target) => {
            // Zip route parameter names with captured URL values
            let mut params = HashMap::new();
            for (name, val) in target.param_names.iter().zip(captured_values.into_iter()) {
                params.insert(name.clone(), UntrustedString::new(val));
            }

            RoutingResult::Found(&*target.handler, target.required_role, params)
        }
        None => {
            if !current.methods.is_empty() {
                RoutingResult::MethodNotAllowed
            } else {
                RoutingResult::NotFound
            }
        }
    }
}
}
