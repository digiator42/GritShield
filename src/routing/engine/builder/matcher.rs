use crate::http::request::HttpMethod;
use crate::routing::engine::{Router, RoutingResult};
use crate::security::xss::UntrustedString;
use std::collections::HashMap;

impl Router {
    pub fn match_route<'a>(&'a self, method: &HttpMethod, path: &str) -> RoutingResult<'a> {
        let mut current = &self.root;
        let mut params = HashMap::new();

        // normalize the string boundaries without altering mid-path structures
        let mut trimmed_path = path;
        if trimmed_path.starts_with('/') {
            trimmed_path = &trimmed_path[1..];
        }
        if trimmed_path.ends_with('/') && !trimmed_path.is_empty() {
            trimmed_path = &trimmed_path[..trimmed_path.len() - 1];
        }

        // Parse segments strictly, reject path if intermediate empty elements exist
        let segments: Vec<&str> = if trimmed_path.is_empty() {
            Vec::new() // Handles the base root "/" path cleanly
        } else {
            let parts: Vec<&str> = trimmed_path.split('/').collect();
            if parts.iter().any(|s| s.is_empty()) {
                // Catches invalid sequences "/admin/////users" or "///"
                return RoutingResult::NotFound;
            }
            parts
        };

        // Process the cleanly generated routing segments
        for (i, segment) in segments.iter().enumerate() {
            if let Some(next_node) = current.children.get(*segment) {
                current = next_node;
            } else {
                // Find any child key that signals a dynamic parameter
                let param_match = current
                    .children
                    .iter()
                    .find(|(key, _)| key.starts_with(':'));

                if let Some((key, param_node)) = param_match {
                    // Check if either the map key OR the internal name contains the '*' wildcard flag
                    let is_wildcard = key.contains('*')
                        || param_node
                            .parameter_name
                            .as_ref()
                            .map_or(false, |name| name.contains('*'));

                    if is_wildcard {
                        // Grab everything remaining, join it with slashes, and clean the parameter key
                        let remainder = segments[i..].join("/");
                        let clean_key = key.trim_start_matches(':').to_string(); // drops ':' to leave '*path'

                        params.insert(clean_key, UntrustedString::new(remainder));
                        current = param_node;
                        break;
                    } else {
                        // Try the node's explicit property first; if None, fall back to the child map key string!
                        let clean_key = if let Some(ref name) = param_node.parameter_name {
                            name.trim_start_matches(':').to_string()
                        } else {
                            key.trim_start_matches(':').to_string()
                        };

                        // Insert the dynamic slug value safely into our parameters dictionary
                        params.insert(clean_key, UntrustedString::new(segment.to_string()));

                        // Advance the tracker node downward to continue evaluating subsequent segments
                        current = param_node;
                    }
                } else {
                    return RoutingResult::NotFound;
                }
            }
        }

        match current.methods.get(method) {
            Some(target) => RoutingResult::Found(&*target.handler, target.required_role, params),
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