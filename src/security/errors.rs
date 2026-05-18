use crate::protocol::response::Response;
use crate::routing::trie::RequestContext;
use crate::security::xss::Sanitizer;
use futures::future::{self, BoxFuture, FutureExt};

pub type ErrorHandlerFn = fn(RequestContext, FrameworkError) -> BoxFuture<'static, Response>;

#[derive(Debug)]
pub enum FrameworkError {
    Panic(String),
    DatabaseFailure(String),
    FormParsingError(String),
    UnauthorizedAccess,
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

pub fn default_framework_error_handler(
    _: RequestContext,
    err: FrameworkError,
) -> BoxFuture<'static, Response> {
    async move {
        let is_production = crate::core::env::get_env("APP_ENV", "development") == "production";
        // Detect the type of error to tailor the presentation layout safely
        let (status_code, title, summary, technical_details) = match err {
            FrameworkError::Panic(msg) => (
                500,
                "Internal Server Error",
                "A critical runtime exception was caught by GritShield's isolation boundary.",
                msg,
            ),
            FrameworkError::DatabaseFailure(msg) => (
                500,
                "Database Connection Error",
                "The storage layer failed to respond safely to the execution pipeline request.",
                msg,
            ),
            FrameworkError::FormParsingError(msg) => (
                400,
                "Bad Request Payload",
                "The incoming structural body encoding could not be parsed safely.",
                msg,
            ),
            FrameworkError::UnauthorizedAccess => (
                401,
                "Unauthorized",
                "Authentication credentials are missing or could not be securely validated.",
                "Access rejected due to missing Session User ID state identifier or invalid JWT token signatures.".to_string(),
            ),
        };

        // Render the luxury default HTML design layout
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
            max-width: 650px;
            width: 100%;
            background: var(--surface);
            padding: 2.5rem;
            border-radius: 12px;
            box-shadow: 0 10px 25px -5px rgba(0, 0, 0, 0.3);
            border-top: 4px solid var(--accent-red);
        }}
        h1 {{
            font-size: 1.8rem;
            margin-top: 0;
            margin-bottom: 0.5rem;
            display: flex;
            align-items: center;
            gap: 0.75rem;
        }}
        p {{
            color: var(--text-muted);
            line-height: 1.6;
        }}
        .code-block {{
            background: #010409;
            padding: 1rem;
            border-radius: 6px;
            font-family: "SFMono-Regular", Consolas, "Liberation Mono", Menlo, monospace;
            font-size: 0.875rem;
            color: #F87171;
            overflow-x: auto;
            border: 1px solid #2D3139;
            margin-top: 1.5rem;
        }}
        .badge {{
            background: rgba(239, 68, 68, 0.15);
            color: var(--accent-red);
            padding: 0.25rem 0.6rem;
            border-radius: 4px;
            font-size: 0.75rem;
            text-transform: uppercase;
            font-weight: bold;
            letter-spacing: 0.05em;
        }}
        .footer {{
            margin-top: 2rem;
            padding-top: 1rem;
            border-top: 1px solid #2D3139;
            font-size: 0.75rem;
            color: var(--text-muted);
            display: flex;
            justify-content: space-between;
        }}
    </style>
</head>
<body>
    <div class="container">
        <h1>
            <span>{title}</span>
            <span class="badge">{status_code}</span>
        </h1>
        <p>{summary}</p>
        
        {details_block}

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
            details_block = if is_production {
                // Production: Completely omit sensitive low-level debug string patterns
                "<p style=\"font-style: italic;\">Diagnostic logs have been recorded securely on the server console shell block.</p>".to_string()
            } else {
                // Development: Provide maximum structural information for local application prototyping
                format!(
                    r#"<div class="code-block"><strong>[Debug Info Trace Window]</strong><br><br>{}</div>"#,
                    technical_details
                )
            }
        );

        Response::new(status_code, Sanitizer::trust(&html_content))
    }.boxed()
}
