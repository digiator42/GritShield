//! How routes are declared and matched.
//!
//! GritShield builds a trie (prefix tree) at startup and walks it one path
//! segment at a time. Three rules follow from that, and they are most of what
//! there is to know about routing:
//!
//! 1. `:name` captures **exactly one** segment.
//! 2. At each segment an exact child wins over a `:param` child.
//! 3. If the node exists but the verb is wrong you get `405`, not `404`.
//!
//! There is no multi-segment wildcard. `:*path` looks like one but is only a
//! parameter that happens to be named `*path`, so it still matches a single
//! segment — see the note on `wildcard_trap` below.
//!
//! Nothing in this file registers a route by hand. `Router::new()` scans the
//! `inventory` registry that `#[controller]` and `#[get]` write into, so these
//! declarations are the whole wiring.

use gritshield::http::response::HttpStatus;
use gritshield::prelude::*;
use serde_json::json;

/// A handler does not have to live in a `#[controller]` block. This one is
/// mounted at the root and costs nothing to declare.
#[get("/")]
pub async fn home() -> Response {
    Response::json(
        HttpStatus::Ok,
        &json!({
            "service": "routing guide",
            "try": [
                "/api/ping",
                "/api/users/42",
                "/api/users/42/posts/7",
                "/api/users/profile",
                "/api/items",
                "/api/search?q=rust&tag=a&tag=b",
            ]
        }),
    )
}

pub struct ApiController;

/// `#[controller("/api")]` supplies a prefix. It is plain string concatenation,
/// **not** path joining, so the sub-paths below must carry their own leading
/// slash — `#[get("ping")]` would register `/apiping`.
#[controller("/api")]
impl ApiController {
    #[get("/ping")]
    pub async fn ping() -> Response {
        Response::json(HttpStatus::Ok, &json!({ "pong": true }))
    }

    /// ## Path parameters
    ///
    /// `:id` captures one segment and lands in `ctx.params` as an
    /// `UntrustedString`. `ctx.param` is the same lookup with the unwrapping
    /// already done.
    ///
    /// ```bash
    /// curl http://127.0.0.1:8081/api/users/42
    /// curl http://127.0.0.1:8081/api/users/<script>
    /// ```
    ///
    /// Note what is *not* happening: the value arrives unescaped, because the
    /// router does not know whether it will end up in HTML, a URL or a log
    /// line. That decision belongs at the point of use — see
    /// `examples/security`.
    #[get("/users/:id")]
    pub async fn user_by_id(ctx: RequestContext) -> Response {
        // `ctx.param` -> Option<&str>; `ctx.params` -> &UntrustedString.
        let id = ctx.param("id").unwrap_or("missing");

        Response::json(
            HttpStatus::Ok,
            &json!({
                "id": id,
                // Present so you can see the raw wrapper type is available too.
                "is_untrusted_type": ctx.params.contains_key("id"),
            }),
        )
    }

    /// Several parameters in one route are fine — they zip against the
    /// captured segments in order.
    ///
    /// ```bash
    /// curl http://127.0.0.1:8081/api/users/42/posts/7
    /// ```
    #[get("/users/:id/posts/:post_id")]
    pub async fn post_by_id(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({
                "user_id": ctx.param("id"),
                "post_id": ctx.param("post_id"),
            }),
        )
    }

    /// ## Route priority
    ///
    /// This is registered *after* `/api/users/:id`, and it still wins for
    /// `/api/users/profile`, because the matcher tries an exact child before it
    /// falls back to a `:param` child. Order of declaration does not matter.
    ///
    /// ```bash
    /// curl http://127.0.0.1:8081/api/users/profile
    /// # {"matched":"exact route, not the :id parameter"}
    ///
    /// curl http://127.0.0.1:8081/api/users/anything-else
    /// # {"matched":"the :id parameter"}
    /// ```
    #[get("/users/profile")]
    pub async fn user_profile() -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({ "matched": "exact route, not the :id parameter" }),
        )
    }

    /// ## Path vs query
    ///
    /// A path parameter is part of *which* resource you want; a query parameter
    /// modifies the representation. `query_param` returns the first value,
    /// `query_all` returns every value for a repeated key.
    ///
    /// ```bash
    /// curl "http://127.0.0.1:8081/api/search?q=rust&tag=a&tag=b"
    /// ```
    #[get("/search")]
    pub async fn search(ctx: RequestContext) -> Response {
        let tags: Vec<&str> = ctx
            .query_all("tag")
            .map(|values| values.iter().map(|t| t.as_str()).collect())
            .unwrap_or_default();

        Response::json(
            HttpStatus::Ok,
            &json!({
                "q": ctx.query_param("q"),
                "tags": tags,
                "path_had_no_parameters": ctx.params.is_empty(),
            }),
        )
    }
}

pub struct ItemController;

/// ## Verbs, and where `405` comes from
///
/// All five verbs share `/api/items/:id`. The trie stores one node per path
/// with a method map underneath, so a path that exists with some verbs but not
/// the requested one answers `405 Method Not Allowed` — which is what lets a
/// client tell "you asked for the wrong verb" apart from "that path is not
/// here".
///
/// ```bash
/// curl -i http://127.0.0.1:8081/api/items/7            # 405, no GET registered
/// curl -i -X PUT    http://127.0.0.1:8081/api/items/7  # 200
/// curl -i -X DELETE http://127.0.0.1:8081/api/items/7  # 200
/// curl -i http://127.0.0.1:8081/api/items             # 200
/// curl -i -X POST   http://127.0.0.1:8081/api/items   # 200
/// ```
#[controller("/api/items")]
impl ItemController {
    #[get("/")]
    pub async fn list() -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({ "items": ["hammer", "spanner"], "count": 2 }),
        )
    }

    #[post("/")]
    pub async fn create(ctx: RequestContext) -> Response {
        let name = ctx.form.get_plain_str("name").unwrap_or("unnamed");

        Response::json(
            HttpStatus::Created,
            &json!({ "created": name, "id": "generated-server-side" }),
        )
    }

    #[put("/:id")]
    pub async fn replace(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({ "verb": "PUT", "replaced": ctx.param("id"), "note": "full replace" }),
        )
    }

    #[patch("/:id")]
    pub async fn update(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({ "verb": "PATCH", "updated": ctx.param("id"), "note": "partial update" }),
        )
    }

    #[delete("/:id")]
    pub async fn remove(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({ "verb": "DELETE", "removed": ctx.param("id") }),
        )
    }
}

/// ## The wildcard trap
///
/// `:*path` reads like a catch-all and is not one. It is a parameter named
/// `*path`, so it still matches a single segment:
///
/// ```bash
/// curl -i http://127.0.0.1:8081/static/logo.png     # 200, bound to *path
/// curl -i http://127.0.0.1:8081/static/css/app.css  # 404, two segments
/// ```
///
/// Note this handler sits outside the `ApiController` block, so it is mounted
/// at `/static/...` and not `/api/static/...` — a `#[controller]` prefix
/// applies only to the block it is written on.
///
/// A `*path` that swallowed several segments would not be able to hand you the
/// remainder of the path as a single value anyway. To serve a directory you
/// either register the routes you need or resolve the path yourself in a
/// handler that owns the whole subtree.
#[get("/static/:*path")]
pub async fn wildcard_trap(ctx: RequestContext) -> Response {
    Response::json(
        HttpStatus::Ok,
        &json!({
            "captured": ctx.param("*path"),
            "segments_matched": 1,
            "note": "this is a single-segment parameter, not a catch-all",
        }),
    )
}