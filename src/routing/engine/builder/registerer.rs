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
    pub fn route<H>(mut self, route_info: (&str, HttpMethod, H)) -> Self
    where
        H: IntoHandler,
    {
        self.add_route(route_info.1, route_info.0, route_info.2, None);
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
        H: IntoHandler,
    {
        let mut current = &mut self.root;

        for segment in path.split('/').filter(|s| !s.is_empty()) {
            current = current
                .children
                .entry(segment.to_string())
                .or_insert(Node::new());
        }

        current.is_end = true;
        // Inject both operational layers into the target method bucket
        current.methods.insert(
            method,
            RouteTarget {
                handler: Box::new(handler),
                required_role,
            },
        );
    }
}