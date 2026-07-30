use crate::http::response::Response;
use crate::routing::engine::RequestContext;
use crate::security::xss::Sanitizer;
use futures::future::{BoxFuture, FutureExt};
use std::backtrace::Backtrace;
use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::RwLock;

pub type ErrorHandlerFn = fn(RequestContext, ShieldError) -> BoxFuture<'static, Response>;
pub type CatchHandlerFn = fn(RequestContext) -> BoxFuture<'static, Response>;

static CATCH_REGISTRY: OnceLock<RwLock<HashMap<u16, CatchHandlerFn>>> = OnceLock::new();

pub fn register_catch(status: u16, handler: CatchHandlerFn) {
    let registry = CATCH_REGISTRY.get_or_init(|| RwLock::new(HashMap::new()));
    if let Ok(mut guard) = registry.write() {
        guard.insert(status, handler);
    }
}

pub fn get_catch(status: u16) -> Option<CatchHandlerFn> {
    let registry = CATCH_REGISTRY.get()?;
    let guard = registry.read().ok()?;
    guard.get(&status).copied()
}

#[derive(Debug)]
pub enum ShieldError {
    Panic {
        message: String,
        backtrace: Backtrace,
    },
    DatabaseFailure {
        message: String,
        backtrace: Backtrace,
    },
    FormParsingError {
        message: String,
        backtrace: Backtrace,
    },
    MethodNotAllowed,
    UnauthorizedAccess,
    BadRequest(String),
    Forbidden,
    NotFound,
    InternalError(String),
    Custom {
        status: u16,
        message: String,
    },
}

impl ShieldError {
    pub fn status_code(&self) -> u16 {
        match self {
            ShieldError::Panic { .. } => 500,
            ShieldError::DatabaseFailure { .. } => 500,
            ShieldError::FormParsingError { .. } => 400,
            ShieldError::MethodNotAllowed => 405,
            ShieldError::UnauthorizedAccess => 401,
            ShieldError::BadRequest(_) => 400,
            ShieldError::Forbidden => 403,
            ShieldError::NotFound => 404,
            ShieldError::InternalError(_) => 500,
            ShieldError::Custom { status, .. } => *status,
        }
    }

    pub fn panic(msg: String) -> Self {
        Self::Panic {
            message: msg,
            backtrace: Backtrace::capture(),
        }
    }

    pub fn database_failure(msg: String) -> Self {
        Self::DatabaseFailure {
            message: msg,
            backtrace: Backtrace::capture(),
        }
    }

    pub fn form_parsing_error(msg: String) -> Self {
        Self::FormParsingError {
            message: msg,
            backtrace: Backtrace::capture(),
        }
    }
}

#[derive(Clone)]
pub struct GlobalErrorHandler {
    pub handler: Option<ErrorHandlerFn>,
}

impl GlobalErrorHandler {
    pub fn new() -> Self {
        Self { handler: None }
    }
}

/// Default framework error handler that checks for custom catches first
pub fn default_framework_error_handler(
    ctx: RequestContext,
    err: ShieldError,
) -> BoxFuture<'static, Response> {
    async move {
        let status = err.status_code();

        // 1. Check if there's a custom catch for this status code
        if let Some(catch_handler) = get_catch(status) {
            return catch_handler(ctx).await;
        }

        // 2. Fallback to HTML error page
        render_html_error(ctx, err, status).await
    }
    .boxed()
}

/// Render the default HTML error page
async fn render_html_error(_: RequestContext, err: ShieldError, status_code: u16) -> Response {
    let is_production = crate::core::env::get_env("APP_ENV", "development") == "production";
    let is_trace_enabled = crate::core::env::get_env("RUST_BACKTRACE", "0") == "1"
        || crate::core::env::get_env("RUST_LIB_BACKTRACE", "0") == "1";

    let (title, summary, technical_details, backtrace) = match err {
        ShieldError::Panic { message, backtrace } => (
            "Internal Server Error",
            "A critical runtime exception was caught by GritShield's isolation boundary.",
            message,
            Some(backtrace),
        ),
        ShieldError::DatabaseFailure { message, backtrace } => (
            "Database Connection Error",
            "The storage layer failed to respond safely to the execution pipeline request.",
            message,
            Some(backtrace),
        ),
        ShieldError::FormParsingError { message, backtrace } => (
            "Bad Request Payload",
            "The incoming structural body encoding could not be parsed safely.",
            message,
            Some(backtrace),
        ),
        ShieldError::MethodNotAllowed => (
            "Method Not Allowed",
            "The HTTP method used is not supported for this endpoint.",
            "The server does not allow the HTTP method specified in the request.".to_string(),
            None,
        ),
        ShieldError::UnauthorizedAccess => (
            "Unauthorized",
            "Authentication credentials are missing or could not be securely validated.",
            "Access rejected due to missing Session User ID state identifier or invalid JWT token signatures.".to_string(),
            None,
        ),
        ShieldError::BadRequest(reason) => (
            "Bad Request",
            "The request could not be understood or was missing required parameters.",
            format!("The server could not process the request due to client error: {}.", reason),
            None,
        ),
        ShieldError::NotFound => (
            "Not Found",
            "The requested resource could not be found on this server.",
            "The server has not found anything matching the Request-URI.".to_string(),
            None,
        ),
        ShieldError::Forbidden => (
            "Forbidden",
            "The server understood the request but refuses to authorize it.",
            "The server has refused to fulfill the request.".to_string(),
            None,
        ),
        ShieldError::InternalError(reason) => (
            "Internal Server Error",
            "An unexpected condition was encountered on the server.",
            reason,
            None,
        ),
        ShieldError::Custom { status, message } => {
            let status_err: &str = format!("Error {}", status).leak();
            (
                status_err,
                "A custom error was encountered.",
                message,
                None,
            )
        },
    };

    let backtrace_html = if !is_production && is_trace_enabled && backtrace.is_some() {
        let bt = backtrace.unwrap();
        let bt_string = format!("{:#?}", bt);
        let trace_lines: Vec<&str> = bt_string.lines().take(50).collect();
        let formatted_trace = trace_lines.join("\n");

        format!(
            r#"<details class="trace-details">
                <summary style="cursor: pointer; color: #3B82F6; margin-top: 1rem;">View Full Stack Trace</summary>
                <div class="code-block" style="max-height: 400px; overflow-y: auto;">
                    <strong>Stack Backtrace:</strong><br/><br/>
                    <pre style="margin: 0; font-family: monospace; font-size: 0.75rem;">{}</pre>
                </div>
            </details>"#,
            html_escape::encode_text(&formatted_trace)
        )
    } else if !is_production {
        r#"<p style="font-style: italic;">💡 Tip: Set RUST_BACKTRACE=1 to see detailed stack traces</p>"#.to_string()
    } else {
        String::new()
    };

    let html_content = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>GritShield Safety Portal - {title}</title>
    <style>
        :root {{
            --bg-main: #0B0F19;
            --surface: #161B26;
            --accent-red: #EF4444;
            --accent-blue: #3B82F6;
            --text-main: #F3F4F6;
            --text-muted: #9CA3AF;
            --trace-bg: #010409;
        }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            background-color: var(--bg-main);
            color: var(--text-main);
            margin: 0;
            padding: 0;
            display: flex;
            align-items: center;
            justify-content: center;
            min-height: 100vh;
        }}
        .container {{
            max-width: 850px;
            width: 100%;
            background: var(--surface);
            padding: 2.5rem;
            border-radius: 12px;
            box-shadow: 0 10px 25px -5px rgba(0, 0, 0, 0.3);
            border-top: 4px solid var(--accent-red);
        }}
        h1 {{ font-size: 1.8rem; margin-top: 0; margin-bottom: 0.5rem; display: flex; align-items: center; gap: 0.75rem; }}
        p {{ color: var(--text-muted); line-height: 1.6; }}
        .code-block {{ background: var(--trace-bg); padding: 1rem; border-radius: 6px; font-family: monospace; font-size: 0.875rem; color: #F87171; overflow-x: auto; border: 1px solid #2D3139; margin-top: 1rem; }}
        .code-block pre {{ margin: 0; white-space: pre-wrap; word-wrap: break-word; }}
        .badge {{ background: rgba(239, 68, 68, 0.15); color: var(--accent-red); padding: 0.25rem 0.6rem; border-radius: 4px; font-size: 0.75rem; text-transform: uppercase; font-weight: bold; letter-spacing: 0.05em; }}
        .footer {{ margin-top: 2rem; padding-top: 1rem; border-top: 1px solid #2D3139; font-size: 0.75rem; color: var(--text-muted); display: flex; justify-content: space-between; }}
        .trace-details {{ margin-top: 1rem; }}
        .trace-details summary {{ cursor: pointer; user-select: none; }}
        .trace-details summary:hover {{ opacity: 0.8; }}
        .request-info {{ font-size: 0.75rem; font-family: monospace; }}
    </style>
</head>
<body>
    <div class="container">
        <h1>
            <span>{title}</span>
            <span class="badge">{status_code}</span>
        </h1>
        <p>{summary}</p>

        <div class="code-block">
            <strong>Error Details:</strong><br/>
            {technical_details}
        </div>

        {backtrace_html}

        <div class="footer">
            <span>Powered by <strong>GritShield Security Framework</strong></span>
            <span>Ref: {status_code}-ERR</span>
        </div>
    </div>
</body>
</html>"#,
        title = title,
        status_code = status_code,
        summary = summary,
        technical_details = html_escape::encode_text(&technical_details),
        backtrace_html = backtrace_html,
    );

    Response::new(status_code, Sanitizer::trust(&html_content))
}
