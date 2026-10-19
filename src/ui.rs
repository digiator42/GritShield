//! The shared view AST — a backend-authored UI that a browser mounts with live
//! reactivity.
//!
//! # The point
//!
//! GritShield owns the server; the browser owns nothing but pixels. The
//! *shared AST* is the seam between them: a view built in Rust on the server
//! becomes **data**, which can be rendered to HTML for the first paint and
//! shipped to a browser as JSON, where a Velo client mounts the very same tree
//! with live fine-grained reactivity.
//!
//! One source, two consumers, no drift — and the server keeps the logic.
//!
//! ```
//! use gritshield::ui::{render_with, Attr, Expr, HtmlOptions, Node, SignalDecl, View};
//!
//! let view = View::new(
//!     "threat-panel",
//!     Node::element("section")
//!         .static_attr("class", "panel")
//!         .child(Node::element("h2").child(Node::text("Live threats")))
//!         .child(Node::text_expr(Expr::signal("count")))
//!         .attr(Attr::class_toggle("hot", Expr::signal("hot"))),
//! )
//! .with_signal(SignalDecl::count("count", 3.0))
//! .with_signal(SignalDecl::scalar("hot", false));
//!
//! let html = render_with(&view, &HtmlOptions::new().with_signal("count", 7.0));
//! assert!(html.contains(">7<"));
//! assert!(html.contains(r#"class="panel""#));
//! ```
//!
//! # Three ways in
//!
//! 1. **Build it.** [`View`] and its builders are plain data. This is the path
//!    that ships: a view built here is serializable and can be handed to a
//!    client verbatim.
//! 2. **Serve it.** [`serve::respond`] picks the representation from the
//!    request's `Accept` header — HTML for a browser, the AST for a client.
//! 3. **Read existing Maud output.** [`migrate::view_from_maud`] recovers a
//!    view tree from markup that Maud already produced, so a dashboard can be
//!    migrated one region at a time rather than all at once.
//!
//! # Feature flag
//!
//! Enabled by the `ui` feature. It adds one dependency (`velo_ast`) and pulls
//! in no browser code — the AST crate is pure data that compiles for both a
//! native server and `wasm32-unknown-unknown`.

pub use velo_ast;

/// The view tree itself.
pub use velo_ast::node::{Element, Node};
/// Attribute bindings.
pub use velo_ast::attr::Attr;
/// Expression trees — every value position.
pub use velo_ast::expr::{ArithOp, CmpOp, Expr};
/// Object/array traversal paths.
pub use velo_ast::path::{ObjPath, PathSeg};
/// Event actions — what happens when a DOM event fires.
pub use velo_ast::event::{EventAction, EventBinding, EventOp, EventValue, ServerRoute};
/// The root payload: a view, its signals, its handlers, its payload.
pub use velo_ast::view::{HandlerRef, SignalDecl, SignalKind, View};
/// Dynamic values.
pub use velo_ast::value::Value;

/// Render a view to an HTML string — the backend's first paint.
pub use velo_ast::render;
/// Render a view with explicit request state (signals, route params, query).
pub use velo_ast::render_with;
/// Render a view plus the seed island a client adopts.
pub use velo_ast::render_document;
/// Render only the seed island.
pub use velo_ast::render_seed_island;
/// Render a view into an HTMX partial.
pub use velo_ast::render_partial;
/// SSR options.
pub use velo_ast::html::{HtmlOptions, SeedIsland};

/// Parse HTML into a tree.
pub use velo_ast::parse_html::parse;
/// Parse HTML into a named [`View`].
pub use velo_ast::parse_html::parse_view;

/// Reading existing markup back into a view — the migration path.
pub mod migrate;

/// Serving a view to a browser — as the AST (interactive) or as markup (first
/// paint), chosen by the request's `Accept` header.
pub mod serve;

/// The AST revision this build speaks. Bump-checked on every payload.
pub use velo_ast::AST_VERSION;
