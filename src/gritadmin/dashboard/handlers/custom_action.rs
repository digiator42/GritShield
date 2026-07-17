use crate::database::repository::registry::{AdminHandlerFn, ACTIONS_REGISTRY};
use crate::gritadmin::dashboard::{error_response, success_response};
use crate::http::response::IntoResponseBody;
use crate::prelude::*;
use std::collections::HashMap;

/// Execute a custom action on selected records (type-erased)
pub async fn handle_custom_action(ctx: RequestContext) -> Response {
    // Get the full path from the request
    let path = ctx.req.uri.clone();

    // Parse table_slug from the path: /admin/{table_slug}/action/{action_name}
    let path_parts: Vec<&str> = path.trim_start_matches("/admin/").split('/').collect();

    // Expect: [table_slug, "action", action_name] or [table_slug, "bulk-action", action_name]
    let table_slug = if path_parts.len() >= 3 {
        path_parts[0].to_string()
    } else {
        return error_response("Invalid action URL format");
    };

    let action_name = match ctx.params.get("action_name").map(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            // Try parsing from path as fallback
            if path_parts.len() >= 3 {
                path_parts[2].to_string()
            } else {
                return error_response("Missing action name");
            }
        }
    };

    // Get the action from the registry
    let action = {
        let registry = ACTIONS_REGISTRY.lock().unwrap();
        registry
            .get(table_slug.as_str())
            .and_then(|actions| actions.iter().find(|a| a.label == action_name))
            .cloned()
    };

    match action {
        Some(action) => {
            let mut res = (action.action)(ctx).await;

            // Extract the raw string response body
            let dev_res = res.body.clone();
            let raw_body_str = dev_res.as_str().unwrap_or("Action completed successfully");

            // Strip HTML tags so the toast gets a clean text notification message
            let mut clean_msg = String::new();
            let mut in_tag = false;
            for c in raw_body_str.chars() {
                if c == '<' {
                    in_tag = true;
                    continue;
                }
                if c == '>' {
                    in_tag = false;
                    continue;
                }
                if !in_tag {
                    clean_msg.push(c);
                }
            }

            let final_msg = clean_msg.trim();

            let trigger = format!(
                r#"{{"showToast": {{"message": "{}", "type": "success"}}}}"#,
                final_msg.replace('"', "\\\"")
            );

            res.headers.push(("hx-trigger".to_string(), trigger));

            // Wipe the body layout so it doesn't try to render on top of your main dashboard structure
            let (body, _) = "".to_string().convert();
            res.body = body;
            res
        }
        None => error_response(format!(
            "Action '{}' not found for table '{}' (available: {:?})",
            action_name,
            table_slug,
            {
                let registry = ACTIONS_REGISTRY.lock().unwrap();
                registry
                    .get(table_slug.as_str())
                    .map(|v| v.iter().map(|a| a.label).collect::<Vec<_>>())
                    .unwrap_or_default()
            }
        )),
    }
}
