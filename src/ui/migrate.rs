//! Reading existing markup back into a view.
//!
//! # Why this exists
//!
//! Maud's `html!` is a `macro_rules!` macro: once it has run, all that is left
//! is a `Markup` string builder. It cannot be intercepted, so existing Maud
//! code cannot become a reactive view by rewiring a macro.
//!
//! What *is* possible is the other direction: take the HTML Maud produced and
//! recover a view tree from it. Structure survives exactly — tags, attributes,
//! text — and the result is a real tree the client can mount, not a blob of
//! raw HTML.
//!
//! # What it does not do
//!
//! Behaviour does not survive, because HTML does not carry it. An `hx-get` in
//! the markup stays a plain attribute; a button that was interactive stays
//! inert until a handler is declared for it.
//!
//! So this is a **migration path, not a conversion**. It lets an existing
//! panel ship to a client as a mountable tree today, and lets handlers be
//! added to it region by region — instead of demanding a rewrite of a
//! dashboard that already works.
//!
//! ```
//! use gritshield::ui::migrate::view_from_markup;
//!
//! let view = view_from_markup("audit_row", r#"<tr class="row"><td>Ada</td></tr>"#)
//!     .expect("well-formed markup");
//!
//! // The tree is real: it can be rendered back, and shipped.
//! assert!(gritshield::ui::render(&view).contains("<td>Ada</td>"));
//! ```

use maud::{Markup, Render};

use crate::ui::View;

/// Read markup into a view tree.
///
/// A malformed fragment does not fail the request: the parser recovers where it
/// can, because a dashboard with one broken panel should still render.
pub fn view_from_markup(name: &str, markup: &str) -> Result<View, String> {
    let root = crate::ui::parse(markup).map_err(|e| e.to_string())?;
    Ok(View::new(name, root))
}

/// Read real Maud output into a view tree.
///
/// Takes the [`Markup`] itself, so the caller does not have to remember to
/// render it first.
pub fn view_from_maud(name: &str, markup: &Markup) -> Result<View, String> {
    // `Markup` renders to a `PreEscaped<String>`; the inner string is the HTML.
    view_from_markup(name, markup.render().0.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use maud::html;

    #[test]
    fn real_maud_output_round_trips_through_the_ast() {
        // Exactly the shape the admin grid produces today.
        let markup = html! {
            tr class="divide-x divide-gray-800" {
                td class="p-3 text-sm" { "ada" }
                td class="p-3 text-sm font-mono" { "login" }
            }
        };

        let view = view_from_maud("audit_row", &markup).expect("parse");
        assert!(view.is_portable());

        let html = crate::ui::render(&view);
        assert!(html.contains("<tr"), "{html}");
        assert!(html.contains("divide-x divide-gray-800"), "{html}");
        assert!(html.contains("ada"), "{html}");
        assert!(html.contains("font-mono"), "{html}");
    }

    #[test]
    fn a_parsed_panel_is_mountable_not_inert() {
        let markup = html! {
            div class="panel" {
                h2 { "Live threats" }
                p { "none" }
            }
        };
        let view = view_from_maud("panel", &markup).expect("parse");
        // Every element became a real node, so a client can mount it. The
        // parser never produces a `Node::Raw`, which is what would make it
        // inert.
        assert!(view.is_portable());
    }

    #[test]
    fn a_malformed_fragment_does_not_fail_the_request() {
        // A panel with a missing end tag must not 500 the dashboard.
        let view = view_from_markup("sloppy", "<div><p>hello").expect("parse");
        assert!(view.is_portable());
    }

    #[test]
    fn a_parsed_panel_survives_the_wire() {
        let markup = html! { div class="panel" { p { "hello" } } };
        let view = view_from_maud("panel", &markup).expect("parse");
        let json = view.to_json().expect("a parsed panel is portable");
        let back = View::from_json(&json).expect("deserialize");
        assert_eq!(back, view);
    }
}
