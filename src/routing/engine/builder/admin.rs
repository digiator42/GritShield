use crate::{http::HttpMethod, info};
use crate::routing::engine::Router;
use crate::routing::AutoRoute;
use colored::*;
// shared dependency if EITHER feature is enabled
#[cfg(any(feature = "swagger", feature = "admin"))]
use crate::database::repository::registry::AdminHandlerFn;
// Swagger-specific items
#[cfg(feature = "swagger")]
use {
    crate::core::swagger::spec::generate_openapi_spec, crate::core::swagger::ui::render_swagger_ui,
};

// Admin-specific items
#[cfg(feature = "admin")]
use {
    crate::database::repository::jql::DynamicColumnSpec,
    crate::database::repository::registry::{ACTIONS_REGISTRY, ADMIN_REGISTRY},
    crate::gritadmin::auth_handlers::{
        handle_login_auth, handle_logout, render_login_page, AdminAuthMiddleware,
    },
    crate::gritadmin::main_handler::*,
    crate::gritadmin::metrics::{
        admin_metrics_api_handler, admin_metrics_html_handler, admin_security_matrix_view_handler,
    },
    crate::prelude::*,
    crate::security::xss::Sanitizer,
    crate::trace,
    crate::log_route,
    std::sync::Arc,
};

fn method_color(method: &str) -> colored::ColoredString {
    match method {
        "GET" => method.green(),
        "POST" => method.blue(),
        "PUT" => method.yellow(),
        "DELETE" => method.red(),
        "PATCH" => method.magenta(),
        _ => method.white(),
    }
}

impl Router {
    pub(crate) fn register_auto_routes(&mut self) {
        let mut max_len = 0;
        let mut all_auto_routes = Vec::new();

        for route in inventory::iter::<AutoRoute> {
            all_auto_routes.push(route);
            let len = route.path.len();
            if len > max_len {
                max_len = len;
            }
        }
        max_len += 4;

        let log_route = |path: &str, max_len: usize, method: &HttpMethod| {
            info!(
                "[DYN-ROUTER] >>: {0:<1$} {2} [{3:<6}]",
                path,
                max_len,
                "->".green(),
                method_color(&format!("{:?}", method))
            );
        };

        for route in all_auto_routes {
            log_route(route.path, max_len, &route.method);

            if let Some(role) = route.required_role {
                self.role_registry.insert(route.path.to_string(), role);
            }

            self.add_route(route.method, route.path, route.handler, route.required_role);
        }
    }

    #[cfg(feature = "admin")]
    pub(crate) fn register_admin_routes(&mut self) {
        let registry = ADMIN_REGISTRY.lock().unwrap();
        let mut all_paths = Vec::new();

        // Collect all paths for alignment
        for (_table_name, model) in registry.iter() {
            all_paths.push(model.route_path.to_string());
            all_paths.push(format!("{}/search", model.route_path));
            all_paths.push(format!("{}/delete", model.route_path));
            all_paths.push(format!("{}/update-cell", model.route_path));
            all_paths.push(format!("{}/query-explorer", model.route_path));
            all_paths.push(format!("{}/:id", model.route_path));
            all_paths.push(format!("{}/bulk-delete", model.route_path));
            all_paths.push(format!("{}/export", model.route_path));
        }

        let static_routes = vec![
            "/admin/api/alter-table/:table_slug/add-column",
            "/admin/dashboard",
            "/admin/api/search-palette",
            "/admin/api/create-table",
            "/admin/api/metrics",
            "/admin/metrics",
            "/admin/settings/security",
            "/admin/login",
            "/admin/api/login",
            "/admin/api/logout",
        ];
        all_paths.extend(static_routes.iter().map(|s| s.to_string()));

        for (table_slug, _) in ACTIONS_REGISTRY.lock().unwrap().iter() {
            all_paths.push(format!("/admin/{}/action/:action_name", table_slug));
            all_paths.push(format!("/admin/{}/bulk-action/:action_name", table_slug));
        }

        let mut max_len = 0;
        for path in &all_paths {
            let len = path.len();
            if len > max_len {
                max_len = len;
            }
        }
        max_len += 4;

        // Register table routes
        for (_table_name, model) in registry.iter() {
            log_route!(model.route_path, max_len, "GET");
            self.add_route(
                HttpMethod::GET,
                model.route_path,
                model.list_handler.clone(),
                None,
            );

            let search = format!("{}/search", model.route_path);
            log_route!(&search, max_len, "GET");
            self.add_route(
                HttpMethod::GET,
                Box::leak(search.into_boxed_str()),
                model.search_handler.clone(),
                None,
            );

            let delete_path = format!("{}/delete", model.route_path);
            log_route!(&delete_path, max_len, "DELETE");
            self.add_route(
                HttpMethod::DELETE,
                Box::leak(delete_path.into_boxed_str()),
                model.delete_handler.clone(),
                None,
            );

            let patch_path = format!("{}/update-cell", model.route_path);
            log_route!(&patch_path, max_len, "PATCH");
            self.add_route(
                HttpMethod::PATCH,
                Box::leak(patch_path.into_boxed_str()),
                model.patch_handler.clone(),
                None,
            );

            let advanced_search_path = format!("{}/query-explorer", model.route_path);
            log_route!(&advanced_search_path, max_len, "GET");
            self.add_route(
                HttpMethod::GET,
                Box::leak(advanced_search_path.into_boxed_str()),
                model.advanced_search_handler.clone(),
                None,
            );

            let detail_path = format!("{}/:id", model.route_path);
            log_route!(&detail_path, max_len, "GET");
            self.add_route(
                HttpMethod::GET,
                Box::leak(detail_path.into_boxed_str()),
                model.detail_handler.clone(),
                None,
            );

            let bulk_path = format!("{}/bulk-delete", model.route_path);
            log_route!(&bulk_path, max_len, "POST");
            self.add_route(
                HttpMethod::POST,
                Box::leak(bulk_path.into_boxed_str()),
                model.bulk_delete_handler.clone(),
                None,
            );

            let export_path = format!("{}/export", model.route_path);
            log_route!(&export_path, max_len, "GET");
            self.add_route(
                HttpMethod::GET,
                Box::leak(export_path.into_boxed_str()),
                model.export_handler.clone(),
                None,
            );

            let bulk_records_path = format!("{}/bulk-create", model.route_path);
            log_route!(&bulk_records_path, max_len, "POST");
            self.add_route(
                HttpMethod::POST,
                Box::leak(bulk_records_path.into_boxed_str()),
                model.bulk_create_records_handler.clone(),
                None,
            );

            let bulk_records_modal_path = format!("{}/bulk-create-modal", model.route_path);
            log_route!(&bulk_records_modal_path, max_len, "GET");
            self.add_route(
                HttpMethod::GET,
                Box::leak(bulk_records_modal_path.into_boxed_str()),
                model.bulk_create_modal_handler.clone(),
                None,
            );
        }

        // Custom action routes
        for (table_slug, _) in ACTIONS_REGISTRY.lock().unwrap().iter() {
            let action_path = format!("/admin/{}/action/:action_name", table_slug);
            let path = Box::leak(action_path.into_boxed_str());
            log_route!(path, max_len, "POST");
            self.add_route(HttpMethod::POST, path, handle_custom_action, None);

            let bulk_action_path = format!("/admin/{}/bulk-action/:action_name", table_slug);
            let bulk_path = Box::leak(bulk_action_path.into_boxed_str());
            log_route!(bulk_path, max_len, "POST");
            self.add_route(HttpMethod::POST, bulk_path, handle_custom_action, None);
        }

        // Dashboard
        let dashboard_handler: AdminHandlerFn = Arc::new(|ctx| Box::pin(handle_dashboard(ctx)));
        log_route!("/admin/dashboard", max_len, "GET");
        self.add_route(HttpMethod::GET, "/admin/dashboard", dashboard_handler, None);

        // Palette
        let palette_handler: AdminHandlerFn = Arc::new(|ctx| Box::pin(handle_search_palette(ctx)));
        log_route!("/admin/api/search-palette", max_len, "GET");
        self.add_route(
            HttpMethod::GET,
            "/admin/api/search-palette",
            palette_handler,
            None,
        );

        // Alter table
        let alter_table_handler: AdminHandlerFn =
            Arc::new(|_ctx| Box::pin(async move { Response::ok("Alter table route".to_string()) }));
        log_route!(
            "/admin/api/alter-table/:table_slug/add-column",
            max_len,
            "POST"
        );
        self.add_route(
            HttpMethod::POST,
            "/admin/api/alter-table/:table_slug/add-column",
            alter_table_handler,
            None,
        );

        // Create table
        let create_table_handler: AdminHandlerFn = Arc::new(|ctx| {
            Box::pin(async move {
                let db = &ctx.db.as_deref().expect("Database connection not mounted");
                let table_name = ctx
                    .form
                    .fields
                    .get("table_name")
                    .cloned()
                    .unwrap_or_default();
                let columns_json = ctx
                    .form
                    .fields
                    .get("columns_data")
                    .cloned()
                    .unwrap_or_default();
                let columns_json_trimmed = columns_json.as_str().trim();

                if columns_json_trimmed.is_empty() {
                    return error_response("Columns configuration cannot be blank.");
                }

                let sanitized_json = if columns_json_trimmed.contains('%') {
                    urlencoding::decode(columns_json_trimmed)
                        .map(|s| s.into_owned())
                        .unwrap_or_else(|_| columns_json_trimmed.to_string())
                } else {
                    columns_json_trimmed.to_string()
                };

                let parsed_columns: Vec<DynamicColumnSpec> =
                    match serde_json::from_str(&sanitized_json) {
                        Ok(cols) => cols,
                        Err(err) => {
                            return error_response(format!("Invalid columns structure: {}", err))
                        }
                    };

                match handle_create_table_dynamic(db, table_name.to_string(), parsed_columns).await
                {
                    Ok(success_msg) => success_response(success_msg),
                    Err(error_msg) => error_response(error_msg),
                }
            })
        });
        log_route!("/admin/api/create-table", max_len, "POST");
        self.add_route(
            HttpMethod::POST,
            "/admin/api/create-table",
            create_table_handler,
            None,
        );

        // Metrics
        log_route!("/admin/api/metrics", max_len, "GET");
        self.add_route(
            HttpMethod::GET,
            "/admin/api/metrics",
            admin_metrics_api_handler,
            None,
        );

        log_route!("/admin/metrics", max_len, "GET");
        self.add_route(
            HttpMethod::GET,
            "/admin/metrics",
            admin_metrics_html_handler,
            None,
        );

        // Security settings
        log_route!("/admin/settings/security", max_len, "GET");
        self.add_route(
            HttpMethod::GET,
            "/admin/settings/security",
            admin_security_matrix_view_handler,
            None,
        );

        // Auth routes
        log_route!("/admin/login", max_len, "GET");
        self.add_route(HttpMethod::GET, "/admin/login", render_login_page, None);

        log_route!("/admin/api/login", max_len, "POST");
        self.add_route(
            HttpMethod::POST,
            "/admin/api/login",
            handle_login_auth,
            None,
        );

        log_route!("/admin/api/logout", max_len, "GET");
        self.add_route(HttpMethod::GET, "/admin/api/logout", handle_logout, None);

        // Admin auth middleware
        self.middlewares.push(Box::new(AdminAuthMiddleware::new()));
    }

    #[cfg(feature = "swagger")]
    pub(crate) fn register_swagger_routes(&mut self) {
        use crate::core::swagger::spec::generate_openapi_spec;
        use crate::core::swagger::ui::render_swagger_ui;
        use crate::database::repository::registry::AdminHandlerFn;
        use crate::http::request::HttpMethod;
        use crate::security::xss::Sanitizer;
        use std::sync::Arc;

        let swagger_handler: AdminHandlerFn = Arc::new(|_ctx| {
            Box::pin(async move {
                let html = render_swagger_ui().into_string();
                Response::ok(Sanitizer::trust(&html))
            })
        });
        self.add_route(HttpMethod::GET, "/admin/docs", swagger_handler, None);

        let openapi_handler: AdminHandlerFn = Arc::new(|_ctx| {
            Box::pin(async move {
                let spec = generate_openapi_spec();
                Response::json(200, &spec)
            })
        });
        self.add_route(
            HttpMethod::GET,
            "/admin/docs/openapi.json",
            openapi_handler,
            None,
        );
    }
}
