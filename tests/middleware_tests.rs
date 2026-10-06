//! Response-phase middleware (`Middleware::on_response`) and the shared
//! response funnel in `connection.rs`.
//!
//! Covered invariants: LIFO ordering over middlewares that executed, header
//! merge on *every* response path (404/405/panic/rejection), short-circuit
//! semantics via the `ran` count, status rewrites landing before the lifecycle
//! hooks observe them, and source compatibility of an `on_request`-only impl.

use gritshield::deps::async_trait;
use gritshield::futures::future::FutureExt;
use gritshield::http::request::HttpMethod;
use gritshield::http::response::Response;
use gritshield::middleware::{AfterRequestHook, Middleware, MiddlewareResult};
use gritshield::routing::engine::{RequestContext, Router};
use gritshield::security::xss::Sanitizer;
use gritshield::testing::WsTestServer;
use std::sync::{Arc, Mutex};

/// Records which phases ran, in order, as `exec:<label>` / `resp:<label>`,
/// and stamps `x-resp-<label>` on the response so the wire output proves the
/// same thing the event log does.
struct TestMiddleware {
    label: &'static str,
    events: Arc<Mutex<Vec<String>>>,
    ctx_header: Option<(&'static str, &'static str)>,
    reject: bool,
    set_status: Option<u16>,
}

impl TestMiddleware {
    fn new(label: &'static str, events: &Arc<Mutex<Vec<String>>>) -> Self {
        Self {
            label,
            events: events.clone(),
            ctx_header: None,
            reject: false,
            set_status: None,
        }
    }

    /// Publishes a header through `ctx.headers`, the channel `on_request` has
    /// for influencing the response — only reaches the wire if the funnel merges it.
    fn with_ctx_header(mut self, key: &'static str, value: &'static str) -> Self {
        self.ctx_header = Some((key, value));
        self
    }

    fn rejecting(mut self) -> Self {
        self.reject = true;
        self
    }

    fn rewriting_status(mut self, status: u16) -> Self {
        self.set_status = Some(status);
        self
    }
}

#[async_trait]
impl Middleware for TestMiddleware {
    async fn on_request(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        self.events
            .lock()
            .unwrap()
            .push(format!("exec:{}", self.label));

        if let Some((key, value)) = self.ctx_header {
            ctx.headers
                .insert(key.to_string(), vec![value.to_string()]);
        }

        if self.reject {
            MiddlewareResult::Error(Response::new(
                403,
                Sanitizer::trust("<h1>403 Forbidden</h1>"),
            ))
        } else {
            MiddlewareResult::Next(None)
        }
    }

    async fn on_response(&self, _ctx: &RequestContext, res: &mut Response) {
        self.events
            .lock()
            .unwrap()
            .push(format!("resp:{}", self.label));

        res.headers
            .push((format!("x-resp-{}", self.label), "1".to_string()));

        if let Some(status) = self.set_status {
            res.status = status;
        }
    }
}

/// Deliberately implements only `on_request` — the shape every pre-`on_response`
/// middleware has, plus the `#[async_trait]` every impl now needs. A defaulted
/// `on_response` must not force a change here: this struct is the
/// source-compatibility guard for the response phase.
struct LegacyMiddleware;

#[async_trait]
impl Middleware for LegacyMiddleware {
    async fn on_request(&self, _ctx: &mut RequestContext) -> MiddlewareResult {
        MiddlewareResult::Next(None)
    }
}

/// Captures the status each `AfterRequestHook` observed.
struct StatusProbe(Arc<Mutex<Vec<u16>>>);

#[async_trait]
impl AfterRequestHook for StatusProbe {
    async fn call(&self, _ctx: &RequestContext, status: u16, _duration: std::time::Duration) {
        self.0.lock().unwrap().push(status);
    }
}

fn events() -> Arc<Mutex<Vec<String>>> {
    Arc::new(Mutex::new(Vec::new()))
}

fn ok_route(router: Router) -> Router {
    router.route((
        "/ok",
        HttpMethod::GET,
        move |_ctx: RequestContext| async move { Response::ok(Sanitizer::trust("ok")) }.boxed(),
    ))
}

async fn panic_handler(_ctx: RequestContext) -> Response {
    panic!("handler exploded");
}

fn event_log(events: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    events.lock().unwrap().clone()
}

fn assert_header(res: &gritshield::testing::WsHttpResponse, name: &str, expected: &str) {
    assert_eq!(
        res.header(name).as_deref(),
        Some(expected),
        "expected header {} on status {}, headers: {:?}",
        name,
        res.status,
        res.headers
    );
}

fn assert_no_header(res: &gritshield::testing::WsHttpResponse, name: &str) {
    assert_eq!(
        res.header(name),
        None,
        "header {} must be absent on status {}, headers: {:?}",
        name,
        res.status,
        res.headers
    );
}

// ==============================================================================
// ORDERING + THE Box FORWARDER
// ==============================================================================

/// Every middleware registered through `add_middleware` is stored as
/// `Box<dyn Middleware>`, so if the blanket `impl` forgets to forward
/// `on_response` the method is silently inherited as a no-op and no stamp
/// ever reaches the wire. This test is that guard.
#[tokio::test]
async fn on_response_runs_after_on_request_and_in_reverse_registration_order() {
    let events = events();
    let router = ok_route(Router::new())
        .add_middleware(TestMiddleware::new("a", &events))
        .add_middleware(TestMiddleware::new("b", &events))
        .add_middleware(TestMiddleware::new("c", &events));

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/ok").await;

    assert_eq!(res.status, 200);
    assert_eq!(
        event_log(&events),
        vec!["exec:a", "exec:b", "exec:c", "resp:c", "resp:b", "resp:a"],
        "request phase runs front to back, response phase unwinds in reverse"
    );

    assert_header(&res, "x-resp-a", "1");
    assert_header(&res, "x-resp-b", "1");
    assert_header(&res, "x-resp-c", "1");
}

/// `Arc<dyn Middleware>` also implements `Middleware` and gets nested inside
/// the `Box` wrapper — both forwarders have to pass through.
#[tokio::test]
async fn arc_wrapped_middleware_forwards_on_response() {
    let events = events();
    let nested: Arc<dyn Middleware> = Arc::new(TestMiddleware::new("nested", &events));
    let router = ok_route(Router::new()).add_middleware(nested);

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/ok").await;

    assert_eq!(res.status, 200);
    assert_header(&res, "x-resp-nested", "1");
    assert!(event_log(&events).contains(&"resp:nested".to_string()));
}

// ==============================================================================
// HEADER MERGE ON EVERY PATH
// ==============================================================================

#[tokio::test]
async fn middleware_headers_reach_404_and_405_responses() {
    let events = events();
    let router = ok_route(Router::new())
        .add_middleware(TestMiddleware::new("mw", &events).with_ctx_header("x-mw", "applied"));

    let server = WsTestServer::start_with_router(router).await;

    let not_found = server.get("/missing").await;
    assert_eq!(not_found.status, 404);
    assert_header(&not_found, "x-mw", "applied");

    let wrong_method = server.request("DELETE", "/ok", &[]).await;
    assert_eq!(wrong_method.status, 405);
    assert_header(&wrong_method, "x-mw", "applied");
}

#[tokio::test]
async fn panic_response_goes_through_the_same_funnel() {
    let events = events();
    let router = Router::new()
        .route((
            "/boom",
            HttpMethod::GET,
            move |ctx: RequestContext| panic_handler(ctx).boxed(),
        ))
        .add_middleware(TestMiddleware::new("mw", &events).with_ctx_header("x-mw", "still-here"));

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/boom").await;

    assert_eq!(res.status, 500);
    assert_header(&res, "x-mw", "still-here");
    assert_header(&res, "x-resp-mw", "1");
}

// ==============================================================================
// SHORT-CIRCUIT SEMANTICS (the `ran` count)
// ==============================================================================

#[tokio::test]
async fn rejection_merges_headers_and_skips_unexecuted_middlewares() {
    let events = events();
    let router = ok_route(Router::new())
        .add_middleware(
            TestMiddleware::new("first", &events).with_ctx_header("x-first", "yes"),
        )
        .add_middleware(TestMiddleware::new("rejector", &events).rejecting())
        .add_middleware(TestMiddleware::new("never", &events));

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/ok").await;

    assert_eq!(res.status, 403);

    // Headers published before the short-circuit reach the rejection response.
    assert_header(&res, "x-first", "yes");

    // Both middlewares that executed got their response phase...
    assert_header(&res, "x-resp-first", "1");
    assert_header(&res, "x-resp-rejector", "1");

    // ...the one behind the short-circuit did not, in either phase.
    assert_no_header(&res, "x-resp-never");
    let log = event_log(&events);
    assert!(!log.contains(&"exec:never".to_string()));
    assert!(!log.contains(&"resp:never".to_string()));
}

/// An `on_request`-only middleware must keep working unchanged.
#[tokio::test]
async fn on_request_only_middleware_still_works() {
    let router = ok_route(Router::new()).add_middleware(LegacyMiddleware);

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/ok").await;

    assert_eq!(res.status, 200);
}

// ==============================================================================
// ORDERING VERSUS THE LIFECYCLE HOOKS
// ==============================================================================

/// `on_response` must run before `run_after_hooks`, otherwise a status it
/// rewrites would be logged and counted as whatever the handler returned.
#[tokio::test]
async fn on_response_status_rewrite_is_what_hooks_observe() {
    let events = events();
    let seen = Arc::new(Mutex::new(Vec::new()));

    let router = ok_route(Router::new())
        .add_middleware(TestMiddleware::new("rewriter", &events).rewriting_status(422))
        .add_after_hook(StatusProbe(seen.clone()));

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/ok").await;

    assert_eq!(res.status, 422, "the wire response reflects the rewrite");
    assert_eq!(
        *seen.lock().unwrap(),
        vec![422],
        "after-hook must observe the rewritten status, not the handler's 200"
    );
}
