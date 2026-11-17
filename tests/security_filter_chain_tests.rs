//! Behavioural tests for the Spring-style `SecurityFilterChain`.
//!
//! These exercise the real connection loop through `WsTestServer`, so chain
//! matching, filter short-circuits, authorization rules, entry points and the
//! response-phase ordering are tested end to end rather than in isolation.

use gritshield::deps::async_trait;
use gritshield::futures::future::FutureExt;
use gritshield::http::request::HttpMethod;
use gritshield::http::response::Response;
use gritshield::middleware::{Middleware, MiddlewareResult};
use gritshield::routing::engine::{RequestContext, Router};
use gritshield::routing::matcher::RequestMatcher;
use gritshield::security::filter_chain::{
    AccessDeniedHandler, AccessRule, AuthenticationEntryPoint, FilterResult, SecurityFilter,
    SecurityFilterChain,
};
use gritshield::security::jwt::{Claims, JwtHandler};
use gritshield::security::xss::Sanitizer;
use gritshield::testing::{WsHttpResponse, WsTestServer};
use std::sync::{Arc, Mutex};

fn ok_route(router: Router, path: &str) -> Router {
    router.route((path, HttpMethod::GET, move |_ctx: RequestContext| {
        async move { Response::ok(Sanitizer::trust("ok")) }.boxed()
    }))
}

fn jwt_token(secret: &str, role: &str) -> String {
    JwtHandler::new(secret)
        .sign(&Claims::new("user-1".to_string(), role.to_string(), 3600))
        .expect("signing a test token")
}

fn signed_session_cookie(response: &WsHttpResponse) -> String {
    let raw = response
        .header("set-cookie")
        .unwrap_or_else(|| panic!("expected Set-Cookie, headers: {:?}", response.headers));
    raw.split(';').next().unwrap().to_string()
}

// ==============================================================================
// MATCHING AND DEFAULT DENY
// ==============================================================================

#[tokio::test]
async fn unmatched_requests_are_untouched() {
    let router = ok_route(Router::new(), "/ok").add_security_chain(
        SecurityFilterChain::builder()
            .matching("/api/**")
            .jwt_auth(JwtHandler::new("secret"))
            .any_request(AccessRule::authenticated())
            .build(),
    );

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/ok").await;

    assert_eq!(res.status, 200);
    assert_eq!(res.body, "ok");
}

#[tokio::test]
async fn protected_route_without_identity_uses_default_entry_point() {
    let router = ok_route(Router::new(), "/api/me").add_security_chain(
        SecurityFilterChain::builder()
            .matching("/api/**")
            .jwt_auth(JwtHandler::new("secret"))
            .any_request(AccessRule::authenticated())
            .build(),
    );

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/api/me").await;

    assert_eq!(res.status, 401);
    assert!(
        res.body.contains("Authentication required"),
        "unexpected body: {}",
        res.body
    );
}

#[tokio::test]
async fn custom_entry_point_can_redirect_to_login() {
    let router = ok_route(Router::new(), "/api/me").add_security_chain(
        SecurityFilterChain::builder()
            .matching("/api/**")
            .jwt_auth(JwtHandler::new("secret"))
            .any_request(AccessRule::authenticated())
            .entry_point(AuthenticationEntryPoint::redirect("/login"))
            .build(),
    );

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/api/me").await;

    assert_eq!(res.status, 303);
    assert_eq!(res.header("location").as_deref(), Some("/login"));
}

// ==============================================================================
// JWT AUTHENTICATION AND ROLE RULES
// ==============================================================================

#[tokio::test]
async fn valid_bearer_token_reaches_the_handler() {
    let secret = "filter-chain-test-secret-key-32";
    let token = jwt_token(secret, "Admin");

    let router = ok_route(Router::new(), "/api/me").add_security_chain(
        SecurityFilterChain::builder()
            .matching("/api/**")
            .jwt_auth(JwtHandler::new(secret))
            .any_request(AccessRule::authenticated())
            .build(),
    );

    let server = WsTestServer::start_with_router(router).await;
    let auth = format!("Bearer {}", token);
    let res = server
        .request("GET", "/api/me", &[("Authorization", &auth)])
        .await;

    assert_eq!(res.status, 200);
}

#[tokio::test]
async fn tampered_bearer_token_is_rejected_even_on_a_public_rule() {
    let router = ok_route(Router::new(), "/api/public/ping").add_security_chain(
        SecurityFilterChain::builder()
            .matching("/api/**")
            .jwt_auth(JwtHandler::new("real-secret"))
            .authorize_public("/api/public/**")
            .build(),
    );

    let server = WsTestServer::start_with_router(router).await;
    let res = server
        .request(
            "GET",
            "/api/public/ping",
            &[("Authorization", "Bearer not.a.valid.token")],
        )
        .await;

    assert_eq!(res.status, 401);
}

#[tokio::test]
async fn role_rules_distinguish_authentication_from_authorization() {
    let secret = "filter-chain-test-secret-key-32";
    let admin = format!("Bearer {}", jwt_token(secret, "Admin"));
    let user = format!("Bearer {}", jwt_token(secret, "User"));

    let router = Router::new()
        .route((
            "/api/admin/panel",
            HttpMethod::GET,
            move |_ctx: RequestContext| {
                async move { Response::ok(Sanitizer::trust("admin")) }.boxed()
            },
        ))
        .route((
            "/api/user/profile",
            HttpMethod::GET,
            move |_ctx: RequestContext| {
                async move { Response::ok(Sanitizer::trust("user")) }.boxed()
            },
        ))
        .add_security_chain(
            SecurityFilterChain::builder()
                .matching("/api/**")
                .jwt_auth(JwtHandler::new(secret))
                .authorize_role("Admin", "/api/admin/**")
                .any_request(AccessRule::authenticated())
                .access_denied(AccessDeniedHandler::Forbidden)
                .build(),
        );

    let server = WsTestServer::start_with_router(router).await;

    let denied = server
        .request("GET", "/api/admin/panel", &[("Authorization", &user)])
        .await;
    assert_eq!(denied.status, 403);

    let allowed = server
        .request("GET", "/api/admin/panel", &[("Authorization", &admin)])
        .await;
    assert_eq!(allowed.status, 200);

    let unrelated = server
        .request("GET", "/api/user/profile", &[("Authorization", &user)])
        .await;
    assert_eq!(unrelated.status, 200);
}

#[tokio::test]
async fn first_matching_rule_wins() {
    let router = ok_route(Router::new(), "/api/admin/open").add_security_chain(
        SecurityFilterChain::builder()
            .matching("/api/**")
            .jwt_auth(JwtHandler::new("secret"))
            .authorize_public("/api/admin/**")
            .authorize_role("Admin", "/api/admin/**")
            .build(),
    );

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/api/admin/open").await;

    assert_eq!(res.status, 200, "the first matching rule is permit-all");
}

#[tokio::test]
async fn rules_fail_closed_for_unmatched_paths() {
    let secret = "filter-chain-test-secret-key-32";
    let admin = format!("Bearer {}", jwt_token(secret, "Admin"));

    let router = ok_route(Router::new(), "/api/anything").add_security_chain(
        SecurityFilterChain::builder()
            .matching("/api/**")
            .jwt_auth(JwtHandler::new(secret))
            .authorize_role("Admin", "/api/admin/**")
            .build(),
    );

    let server = WsTestServer::start_with_router(router).await;
    let res = server
        .request("GET", "/api/anything", &[("Authorization", &admin)])
        .await;

    assert_eq!(
        res.status, 403,
        "a chain with rules must not leave uncovered paths open"
    );
}

#[tokio::test]
async fn method_specific_rules_do_not_cover_other_methods() {
    let secret = "filter-chain-test-secret-key-32";
    let user = format!("Bearer {}", jwt_token(secret, "User"));

    let router = ok_route(Router::new(), "/api/items").add_security_chain(
        SecurityFilterChain::builder()
            .matching("/api/**")
            .jwt_auth(JwtHandler::new(secret))
            .authorize(
                RequestMatcher::path_and_method("/api/items", HttpMethod::POST),
                AccessRule::has_role("Admin"),
            )
            .any_request(AccessRule::authenticated())
            .build(),
    );

    let server = WsTestServer::start_with_router(router).await;

    let list = server
        .request("GET", "/api/items", &[("Authorization", &user)])
        .await;
    assert_eq!(list.status, 200);

    let create = server
        .request("POST", "/api/items", &[("Authorization", &user)])
        .await;
    assert_eq!(create.status, 403);
}

// ==============================================================================
// SESSIONS, CSRF AND THE RESPONSE PHASE
// ==============================================================================

#[tokio::test]
async fn session_login_and_csrf_guard_work_together() {
    let router = Router::new()
        .route((
            "/auth/login",
            HttpMethod::POST,
            move |ctx: RequestContext| {
                async move {
                    ctx.login_user_id("ada");
                    ctx.set_session_data("role", "Admin");
                    Response::ok(Sanitizer::trust("logged-in"))
                }
                .boxed()
            },
        ))
        .route((
            "/auth/token",
            HttpMethod::GET,
            move |ctx: RequestContext| {
                async move { Response::ok(Sanitizer::trust(&ctx.get_csrf_token())) }.boxed()
            },
        ))
        .route((
            "/account/update",
            HttpMethod::POST,
            move |_ctx: RequestContext| {
                async move { Response::ok(Sanitizer::trust("updated")) }.boxed()
            },
        ))
        .add_security_chain(
            SecurityFilterChain::builder()
                .matching("/**")
                .session_auth()
                .csrf_with(|csrf| csrf.ignore_path("/auth/login"))
                .authorize_public("/auth/**")
                .authorize_authenticated("/account/**")
                .entry_point(AuthenticationEntryPoint::redirect("/auth/login"))
                .build(),
        );

    let server = WsTestServer::start_with_router(router).await;

    let login = server.request("POST", "/auth/login", &[]).await;
    assert_eq!(login.status, 200, "login POST is exempt from CSRF");
    let cookie = signed_session_cookie(&login);

    let token_response = server
        .request("GET", "/auth/token", &[("Cookie", &cookie)])
        .await;
    assert_eq!(token_response.status, 200);
    let csrf_token = token_response.body;
    assert!(!csrf_token.is_empty(), "session must expose a CSRF token");

    let rejected = server
        .request("POST", "/account/update", &[("Cookie", &cookie)])
        .await;
    assert_eq!(rejected.status, 403, "missing CSRF token must fail closed");

    let accepted = server
        .request(
            "POST",
            "/account/update",
            &[("Cookie", &cookie), ("X-CSRF-Token", &csrf_token)],
        )
        .await;
    assert_eq!(accepted.status, 200);
}

/// Records request/response ordering across the two pipeline stages.
struct Recorder {
    label: &'static str,
    events: Arc<Mutex<Vec<String>>>,
    reject: bool,
}

#[async_trait]
impl SecurityFilter for Recorder {
    fn name(&self) -> &'static str {
        self.label
    }

    async fn on_request(&self, _ctx: &mut RequestContext) -> FilterResult {
        self.events
            .lock()
            .unwrap()
            .push(format!("exec:{}", self.label));

        if self.reject {
            FilterResult::Respond(Response::new(418, Sanitizer::trust("<h1>teapot</h1>")))
        } else {
            FilterResult::Continue
        }
    }

    async fn on_response(&self, _ctx: &RequestContext, res: &mut Response) {
        self.events
            .lock()
            .unwrap()
            .push(format!("resp:{}", self.label));
        res.headers
            .push((format!("x-resp-{}", self.label), "1".to_string()));
    }
}

struct RecorderMiddleware {
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl Middleware for RecorderMiddleware {
    async fn on_request(&self, _ctx: &mut RequestContext) -> MiddlewareResult {
        self.events.lock().unwrap().push("exec:mw".to_string());
        MiddlewareResult::Next(None)
    }

    async fn on_response(&self, _ctx: &RequestContext, _res: &mut Response) {
        self.events.lock().unwrap().push("resp:mw".to_string());
    }
}

#[tokio::test]
async fn security_response_phase_unwinds_before_middleware() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let handler_events = events.clone();

    let router = Router::new()
        .route(("/orders", HttpMethod::GET, move |_ctx: RequestContext| {
            let events = handler_events.clone();
            async move {
                events.lock().unwrap().push("handler".to_string());
                Response::ok(Sanitizer::trust("orders"))
            }
            .boxed()
        }))
        .add_middleware(RecorderMiddleware {
            events: events.clone(),
        })
        .add_security_chain(
            SecurityFilterChain::builder()
                .matching("/orders")
                .filter(Recorder {
                    label: "a",
                    events: events.clone(),
                    reject: false,
                })
                .filter(Recorder {
                    label: "b",
                    events: events.clone(),
                    reject: false,
                })
                .build(),
        );

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/orders").await;

    assert_eq!(res.status, 200);
    assert_eq!(
        events.lock().unwrap().clone(),
        vec!["exec:mw", "exec:a", "exec:b", "handler", "resp:b", "resp:a", "resp:mw"],
        "middleware wraps the security stage; each stage unwinds in reverse"
    );
    assert_eq!(res.header("x-resp-a").as_deref(), Some("1"));
    assert_eq!(res.header("x-resp-b").as_deref(), Some("1"));
}

#[tokio::test]
async fn short_circuit_runs_only_the_filters_that_executed() {
    let events = Arc::new(Mutex::new(Vec::new()));

    let router = ok_route(Router::new(), "/orders").add_security_chain(
        SecurityFilterChain::builder()
            .matching("/orders")
            .filter(Recorder {
                label: "first",
                events: events.clone(),
                reject: false,
            })
            .filter(Recorder {
                label: "rejector",
                events: events.clone(),
                reject: true,
            })
            .filter(Recorder {
                label: "never",
                events: events.clone(),
                reject: false,
            })
            .build(),
    );

    let server = WsTestServer::start_with_router(router).await;
    let res = server.get("/orders").await;

    assert_eq!(res.status, 418);
    assert_eq!(
        events.lock().unwrap().clone(),
        vec!["exec:first", "exec:rejector", "resp:rejector", "resp:first"],
        "the filter after the short-circuit must not run in either phase"
    );
    assert_eq!(res.header("x-resp-first").as_deref(), Some("1"));
    assert_eq!(res.header("x-resp-rejector").as_deref(), Some("1"));
    assert_eq!(res.header("x-resp-never"), None);
}

// ==============================================================================
// CHAIN ORDER
// ==============================================================================

#[tokio::test]
async fn first_matching_chain_wins() {
    let router = ok_route(Router::new(), "/api/secret")
        .route((
            "/admin/dashboard",
            HttpMethod::GET,
            move |_ctx: RequestContext| {
                async move { Response::ok(Sanitizer::trust("admin")) }.boxed()
            },
        ))
        .add_security_chain(
            SecurityFilterChain::builder()
                .name("api")
                .matching("/api/**")
                .jwt_auth(JwtHandler::new("secret"))
                .any_request(AccessRule::authenticated())
                .build(),
        )
        .add_security_chain(
            SecurityFilterChain::builder()
                .name("admin")
                .matching("/admin/**")
                .any_request(AccessRule::permit_all())
                .build(),
        );

    let server = WsTestServer::start_with_router(router).await;

    assert_eq!(server.get("/api/secret").await.status, 401);
    assert_eq!(server.get("/admin/dashboard").await.status, 200);
}
