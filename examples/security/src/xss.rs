//! XSS prevention, end to end.
//!
//! The rule this module exists to teach: **input is untrusted, output must be
//! encoded once, at the boundary where it becomes HTML.**
//!
//! GritShield gives you two places to do that, and they solve different
//! problems:
//!
//! - [`Sanitizer::encode`] for a value about to be interpolated into a template
//!   or an HTML response. This is the one you reach for most often.
//! - `#[derive(GritSanitizer)]` for a whole request payload, when you want
//!   normalisation rules (`trim`, `lowercase`, ...) applied field by field
//!   declaratively instead of by hand.
//!
//! Open `src/xss.rs` next to this file: it is the guide.

use gritshield::http::response::HttpStatus;
use gritshield::prelude::*;
use gritshield::security::sanitizer::GritSanitizable;
use gritshield::GritSanitizer;
use serde::{Deserialize, Serialize};
use serde_json::json;

/// A request payload whose fields each declare how they should be cleaned.
///
/// The derive runs `sanitize()` in place. Note what it does *not* do: it does
/// not make a value safe to render as HTML. `html_escape` here is a
/// normalisation choice for a field you intend to store and re-render later.
/// For one-off rendering, `Sanitizer::encode` at the point of output is the
/// clearer option, because the escaping then happens where the HTML is built.
#[derive(Debug, Deserialize, Serialize, GritSanitizer)]
pub struct ProfilePayload {
    /// `"  Alice  "` -> `"Alice"`, `"USER@ExAmPlE.com"` -> `"user@example.com"`
    #[clean(trim, lowercase)]
    pub email: String,

    /// `"<script>alert(1)</script>"` -> `"&lt;script&gt;alert(1)&lt;/script&gt;"`
    #[clean(trim, html_escape)]
    pub display_name: String,

    /// `"hello%20world"` -> `"hello world"`
    #[clean(url_decode)]
    pub tagline: String,

    /// Nested DTOs are cleaned recursively.
    #[clean(nested)]
    pub address: Address,

    /// No `#[clean(...)]` means the field is left exactly as it arrived.
    pub age: u8,
}

#[derive(Debug, Deserialize, Serialize, GritSanitizer)]
pub struct Address {
    #[clean(trim, uppercase)]
    pub country: String,
}

pub struct XssController;

#[controller("/xss")]
impl XssController {
    /// ## The trap
    ///
    /// `String` and `&'static str` are sent to the client **as trusted HTML**.
    /// That is the right default for your own markup, and exactly the wrong
    /// default for a value that came off the wire — so this endpoint
    /// interpolates a query parameter straight into a page and lets you watch
    /// the script execute.
    ///
    /// ```bash
    /// curl "http://localhost:8080/xss/unsafe?name=%3Cscript%3Ealert(1)%3C/script%3E"
    /// ```
    #[get("/unsafe")]
    pub async fn render_unsafe(ctx: RequestContext) -> Response {
        let name = ctx.query_param("name").unwrap_or("world");

        // WRONG. `name` is attacker-controlled and goes out as live markup.
        Response::ok(format!("<h1>Hello, {name}</h1>"))
    }

/// ## The fix
    ///
    /// Identical endpoint, one difference: the **value** is encoded, and only the
    /// value. Your own markup stays markup.
    ///
    /// ```bash
    /// curl "http://127.0.0.1:8080/xss/safe?name=%3Cscript%3Ealert(1)%3C/script%3E"
    /// # <h1>Hello, &lt;script&gt;alert(1)&lt;/script&gt;</h1>
    /// ```
    #[get("/safe")]
    pub async fn render_safe(ctx: RequestContext) -> Response {
        let name = ctx.query_param("name").unwrap_or("world");

        // RIGHT. `SafeHtml` renders as text, so the tag arrives as visible
        // characters instead of an element the browser executes.
        let safe_name = Sanitizer::encode(name);

        // Safe because the *only* interpolated part is now inert. The template
        // around it is ours. This is the pattern to reach for.
        Response::ok(format!("<h1>Hello, {safe_name}</h1>"))
    }

    /// ## The other mistake
    ///
    /// Encoding the whole rendered document is also safe, and also wrong — it
    /// escapes your own markup too, so the user sees literal `<h1>` characters
    /// instead of a heading. It is worth hitting once to recognise the symptom:
    /// security fine, page broken.
    ///
    /// ```bash
    /// curl "http://127.0.0.1:8080/xss/over-escaped?name=Alice"
    /// # &lt;h1&gt;Hello, Alice&lt;/h1&gt;
    /// ```
    #[get("/over-escaped")]
    pub async fn render_over_escaped(ctx: RequestContext) -> Response {
        let name = ctx.query_param("name").unwrap_or("world");

        // Correct security, wrong layer. Encode leaves, not the document.
        Response::ok(Sanitizer::encode(&format!("<h1>Hello, {name}</h1>")))
    }

/// `SafeHtml` is the only body type that is *not* re-wrapped in
    /// `Sanitizer::trust`, so returning it directly is another way to mark a
    /// fragment as already-safe. Here there is nothing dynamic, so the string
    /// form is enough.
    #[get("/trusted")]
    pub async fn render_trusted(ctx: RequestContext) -> Response {
        let heading = Sanitizer::trust("<h1>This markup is ours, so it is trusted</h1>");

        Response::ok(format!("{heading}<p>Query was: {}</p>", ctx.req.uri))
    }

    /// ## Declarative payload cleaning
    ///
    /// The response echoes both what you sent and what the fields became, so
    /// the effect of every attribute is visible in one call:
    ///
    /// ```bash
    /// curl -X POST http://localhost:8080/xss/profile \
    ///   -H 'content-type: application/json' \
    ///   -d '{"email":"  USER@ExAmPlE.com ","display_name":"<b>Alice</b>",
    ///        "tagline":"hello%20world","address":{"country":" pt "},"age":31}'
    /// ```
    ///
    /// ```json
    /// {"cleaned":{"address":{"country":"PT"},"age":31,
    ///   "display_name":"&lt;b&gt;Alice&lt;&#x2F;b&gt;",
    ///   "email":"user@example.com","tagline":"hello world"}}
    /// ```
    ///
    /// `email` lost its padding and case, `display_name` was escaped,
    /// `tagline` was url-decoded, `country` was trimmed and upper-cased through
    /// the nested struct, and `age` — which declares no attributes — is
    /// untouched.
    ///
    /// No CSRF token is needed here because `/xss/profile` is in
    /// `public_paths`, and public routes skip the guard entirely. See the note
    /// on `PUBLIC_PATHS` in `main.rs`.
    #[post("/profile")]
    pub async fn clean_profile(ctx: RequestContext) -> Response {
        let mut payload = match ctx.json::<ProfilePayload>().await {
            Ok(parsed) => parsed,
            Err(err) => {
                return Response::json(
                    HttpStatus::BadRequest,
                    &json!({ "error": format!("{err:?}") }),
                )
            }
        };

        Response::json(
            HttpStatus::Ok,
            &json!({
                "cleaned": payload,
                "note": "compare with the raw payload you sent",
            }),
        )
    }

    /// ## Encoding for a non-HTML context
    ///
    /// HTML-escaping is wrong inside a URL or a JSON string. `Sanitizer` has a
    /// dedicated pair for that, so a value destined for a query string is not
    /// mangled by rules meant for markup.
    #[get("/url")]
    pub async fn url_context(ctx: RequestContext) -> Response {
        let raw = ctx.query_param("q").unwrap_or("rust & cargo");

        Response::json(
            HttpStatus::Ok,
            &json!({
                "raw": raw,
                "url_encoded": Sanitizer::url_encode(raw),
                "html_encoded_wrong_here": Sanitizer::encode(raw).to_string(),
            }),
        )
    }
}