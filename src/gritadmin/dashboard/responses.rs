use crate::http::response::Response;
use crate::security::errors::ShieldError;
use crate::security::xss::Sanitizer;
use crate::prelude::*;

pub fn error_response(msg: impl ToString) -> Response {
    let msg = msg.to_string();
    let mut res = Response::new(400, Sanitizer::trust(&msg));
    // Set HX-Trigger header to show a toast
    let trigger = format!(
        r#"{{"showToast": {{"message": "{}", "type": "error"}}}}"#,
        msg.replace('"', "\\\"")
    );
    res.headers.push(("hx-trigger".to_string(), trigger));
    res
}

pub fn success_response(msg: impl ToString) -> Response {
    let msg = msg.to_string();
    let mut res = Response::new(200, Sanitizer::trust(&msg));

    // Optional: Trigger a success toast using hx-trigger headers if your UI uses them
    let trigger = format!(
        r#"{{"showToast": {{"message": "Table created successfully!", "type": "success"}}}}"#
    );
    res.headers.push(("hx-trigger".to_string(), trigger));
    res
}

pub fn shield_error_response(err: ShieldError) -> Response {
    let msg = match err {
        ShieldError::BadRequest(s) => s,
        ShieldError::NotFound => "Resource not found".to_string(),
        ShieldError::UnauthorizedAccess => "Unauthorized".to_string(),
        ShieldError::Forbidden => "Forbidden".to_string(),
        _ => "Internal server error".to_string(),
    };
    error_response(msg)
}
