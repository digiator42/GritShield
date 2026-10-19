//! Serving a view to a browser.
//!
//! # Two responses, one view
//!
//! A [`View`] can be delivered two ways, and the same view produces both:
//!
//! - **JSON** — the AST itself, which a Velo client mounts with live
//!   fine-grained reactivity. This is the interactive path.
//! - **HTML** — the server-rendered markup plus a seed island carrying the
//!   signal values. This is the first paint, and it needs no JavaScript to be
//!   correct.
//!
//! Which one a request gets is decided by the `Accept` header: a browser asking
//! for `text/html` gets markup, a client asking for `application/json` gets the
//! AST. A Velo client that wants the interactive path asks for JSON and mounts
//! it; a plain browser gets HTML that already looks right.
//!
//! ```ignore
//! use gritshield::prelude::*;
//!
//! fn threat_panel() -> gritshield::ui::View {
//!     gritshield::ui::panels::records_view("threats", vec![])
//! }
//!
//! #[get("/admin/threats/panel")]
//! async fn threat_panel_route(ctx: RequestContext) -> Response {
//!     gritshield::ui::serve::respond(&threat_panel(), &ctx)
//! }
//! ```
//!
//! # Why the two can never drift
//!
//! Both come from the same [`View`]. There is no second template to keep in
//! sync, and no chance of the server rendering one thing while the client
//! renders another — the client is rendering the very tree the server sent.

use crate::error;
use crate::http::response::{HttpStatus, Response};
use crate::prelude::RequestContext;
use crate::ui::{render_document, HtmlOptions, View};

/// The `Accept` header value that asks for the AST.
pub const JSON_ACCEPT: &str = "application/json";

/// Serve `view`, choosing JSON or HTML from the request's `Accept` header.
///
/// A client that accepts both — a browser's default `Accept` includes
/// `application/json` only rarely, but a `fetch` call often sends `*/*` — gets
/// HTML unless it explicitly asks for JSON. That keeps the default path the
/// one that works with no JavaScript.
pub fn respond(view: &View, ctx: &RequestContext) -> Response {
    if wants_json(ctx) {
        respond_json(view)
    } else {
        respond_html(view, ctx)
    }
}

/// `true` when the request explicitly prefers JSON.
pub fn wants_json(ctx: &RequestContext) -> bool {
    let Some(accept) = ctx.header("accept") else {
        return false;
    };
    // `application/json` must be present *and* the client must not also be
    // asking for HTML first — a browser's `text/html,application/xhtml+xml,...`
    // must not be read as a request for JSON.
    let accept = accept.to_ascii_lowercase();
    if !accept.contains(JSON_ACCEPT) {
        return false;
    }
    let html_first = accept
        .split(',')
        .map(|part| part.split(';').next().unwrap_or("").trim())
        .find(|part| !part.is_empty() && *part != "*/*");
    matches!(html_first, Some(part) if part == JSON_ACCEPT)
}

/// Serve the AST as JSON — the interactive path.
pub fn respond_json(view: &View) -> Response {
    // Check first so an unserializable view produces a clear server error
    // rather than a confusing one from deep inside the serializer.
    if let Err(e) = view.to_json() {
        error!(
            "gritshield::ui: view `{}` is not serializable: {e}",
            view.name
        );
        return Response::json(
            HttpStatus::InternalServerError,
            &serde_json::json!({
                "error": "view is not serializable",
                "view": view.name,
                "detail": e.to_string(),
            }),
        );
    }
    Response::json_ok(view)
}

/// Serve the server-rendered markup plus the seed island — the first paint.
pub fn respond_html(view: &View, ctx: &RequestContext) -> Response {
    let opts = HtmlOptions::new()
        .with_path(ctx.req.path.clone())
        .with_query_params(query_params(ctx));
    // The AST renderer escapes every text run and every attribute value, so
    // the markup it produces is already safe to trust. `Response::ok(String)`
    // marks it as such — the alternative, `SafeHtml::encode`, would escape it
    // a second time and show the user `&amp;` where they meant `&`.
    Response::ok(render_document(view, &opts))
}

/// The request's query parameters, flattened to `(name, first value)` pairs.
///
/// `UntrustedString` is a marker for "this came off the wire"; the value is
/// escaped on the way into the markup, so reading it here is safe.
fn query_params(ctx: &RequestContext) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (name, values) in &ctx.req.query {
        if let Some(first) = values.first() {
            out.push((name.clone(), first.as_str().to_string()));
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_with_accept(accept: &str) -> RequestContext {
        let mut ctx = RequestContext::new();
        // `RequestContext::header` reads `ctx.headers` (lower-cased keys), not
        // `ctx.req.headers`, so the header has to be set in both places to
        // model a real request.
        ctx.headers
            .insert("accept".to_string(), vec![accept.to_string()]);
        ctx.req
            .headers
            .insert("accept".to_string(), vec![accept.to_string()]);
        ctx
    }

    /// A small view standing in for a real panel.
    fn view() -> View {
        use crate::ui::{Attr, Expr, Node, SignalDecl};
        View::new(
            "audit-log",
            Node::element("section")
                .static_attr("id", "panel")
                .static_attr("class", "panel")
                .attr(Attr::class_toggle("is-on", Expr::signal("hot")))
                .child(Node::element("span").static_attr("id", "count-value").child(
                    Node::text_expr(Expr::signal("count")),
                )),
        )
        .with_signal(SignalDecl::count("count", 1.0))
        .with_signal(SignalDecl::scalar("hot", false))
    }

    #[test]
    fn a_json_request_gets_the_ast() {
        let ctx = ctx_with_accept("application/json");
        assert!(wants_json(&ctx));
    }

    #[test]
    fn a_browser_gets_markup_even_though_json_is_acceptable() {
        // A browser's Accept lists HTML first. Reading that as "give me JSON"
        // would ship a browser raw AST, which is exactly the wrong outcome.
        let ctx = ctx_with_accept("text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8");
        assert!(!wants_json(&ctx));
    }

    #[test]
    fn an_explicit_json_preference_wins_over_a_wildcard() {
        let ctx = ctx_with_accept("application/json, */*;q=0.8");
        assert!(wants_json(&ctx));
    }

    #[test]
    fn a_wildcard_alone_is_not_a_json_request() {
        let ctx = ctx_with_accept("*/*");
        assert!(!wants_json(&ctx));
    }

    #[test]
    fn no_accept_header_means_markup() {
        let ctx = RequestContext::new();
        assert!(!wants_json(&ctx));
    }

    #[test]
    fn the_json_response_carries_the_ast() {
        let response = respond_json(&view());
        assert_eq!(response.status, 200);
        let body = response.body.as_str().unwrap_or_default();
        assert!(body.contains("audit-log"), "{body}");
        // The AST, not the markup: a client that asked for JSON gets the tree.
        assert!(body.contains("\"root\""), "{body}");
        assert!(body.contains("\"signals\""), "{body}");
    }

    #[test]
    fn the_html_response_carries_markup_and_the_seed_island() {
        let mut ctx = RequestContext::new();
        ctx.req.path = "/admin/audit_log".to_string();
        let response = respond_html(&view(), &ctx);
        assert_eq!(response.status, 200);
        let body = response.body.as_str().unwrap_or_default();
        assert!(body.contains("velo-view-seeds"), "{body}");
        assert!(body.contains(r#"id="panel""#), "{body}");
        // The first paint is real markup, not a JSON blob.
        assert!(body.contains("<section"), "{body}");
    }

    #[test]
    fn an_unserializable_view_reports_a_server_error_instead_of_a_broken_body() {
        use crate::ui::{Expr, Node};
        let view = View::new(
            "client-only",
            Node::element("p").child(Node::text_expr(Expr::native(|_| {
                crate::ui::Value::Str("x".into())
            }))),
        );
        let response = respond_json(&view);
        assert_eq!(response.status, 500);
    }
}
