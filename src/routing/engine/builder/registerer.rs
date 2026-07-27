use crate::database::repository::registry::AdminHandlerFn;
use crate::http::request::HttpMethod;
use crate::routing::engine::route::RouteTarget;
use crate::routing::engine::{Node, Router};
use crate::routing::IntoHandler;

impl Router {
    /// Register routes using tuples for a clean, declarative style.
    ///
    /// ### Example
    /// You can declare a matrix of routes in one place:
    /// ```rust,no_run,ignore
    /// let app_routes = vec![
    ///     ("/login",    HttpMethod::GET,  handle_login),
    ///     ("/register", HttpMethod::POST, handle_register),
    ///     ("/dashboard",HttpMethod::GET,  handle_dashboard),
    /// ];
    /// ```
    ///
    /// Then register them all elegantly in a single line:
    /// ```rust,no_run,ignore
    /// app_routes.into_iter().for_each(|r| router.route(r));
    /// ```
    pub fn route<S, H>(mut self, route_info: (S, HttpMethod, H)) -> Self
    where
        S: AsRef<str>,
        H: IntoHandler + 'static,
    {
        self.add_route(route_info.1, route_info.0.as_ref(), route_info.2, None);
        self
    }

    // Update the basic registration engine signature
    pub fn add_route<H>(
        &mut self,
        method: HttpMethod,
        path: &str,
        handler: H,
        required_role: Option<&'static str>,
    ) where
        H: IntoHandler + 'static,
    {
        let path_trimmed = path.trim_matches('/');
        let segments: Vec<&str> = if path_trimmed.is_empty() {
            Vec::new()
        } else {
            path_trimmed.split('/').collect()
        };

        let mut current = &mut self.root;
        let mut param_names = Vec::new();

        for segment in segments {
            if segment.starts_with(':') {
                // Collect the exact parameter name declared in this route's path macro
                let clean_param = segment.trim_start_matches(':').to_string();
                param_names.push(clean_param);

                // Reuse any existing dynamic key branch (e.g. ':id') to keep the tree merged
                let existing_param_key = current
                    .children
                    .keys()
                    .find(|k| k.starts_with(':'))
                    .cloned();

                let node_key = match existing_param_key {
                    Some(key) => key,
                    None => segment.to_string(),
                };

                current = current.children.entry(node_key).or_default();
            } else {
                // Traverse static path segments
                current = current.children.entry(segment.to_string()).or_default();
            }
        }

        // Store the handler along with its specific parameter names list
        current.methods.insert(
            method,
            RouteTarget {
                handler: Box::new(handler),
                required_role,
                param_names,
            },
        );
    }
}
