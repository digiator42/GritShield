//! `#[security_chain]` on free functions: compile-time discovery, ordering and
//! automatic application by `Router::new()`.

use gritshield::http::HttpMethod;
use gritshield::prelude::*;
use gritshield::testing::WsTestServer;

#[security_chain(name = "overlap", order = 5)]
fn declared_overlap() -> SecurityFilterChain {
    SecurityFilterChain::builder()
        .matching("/overlap/**")
        .any_request(AccessRule::permit_all())
        .build()
}

#[security_chain(name = "declared-api", order = 10)]
fn declared_api() -> SecurityFilterChain {
    SecurityFilterChain::builder()
        .matching("/declared/**")
        .authorize_public("/declared/public/**")
        .any_request(AccessRule::deny_all())
        .build()
}

#[security_chain(order = 20)]
fn declared_web() -> SecurityFilterChain {
    SecurityFilterChain::builder()
        .matching("/declared-web/**")
        .any_request(AccessRule::permit_all())
        .build()
}

#[security_chain(name = "overlap-deny", order = 100)]
fn declared_overlap_deny() -> SecurityFilterChain {
    SecurityFilterChain::builder()
        .matching("/overlap/**")
        .any_request(AccessRule::deny_all())
        .build()
}

fn ok(router: Router, path: &str) -> Router {
    router.route((path, HttpMethod::GET, |_ctx: RequestContext| async {
        Response::ok(Sanitizer::trust("ok"))
    }))
}

#[test]
fn router_collects_declared_chains_in_order() {
    let router = Router::new();
    let names: Vec<&str> = router
        .security_chains
        .iter()
        .map(|chain| chain.name())
        .collect();

    assert_eq!(
        names,
        vec!["overlap", "declared-api", "declared_web", "overlap-deny"]
    );
}

#[tokio::test]
async fn declared_chains_are_applied_without_router_changes() {
    let router = ok(Router::new(), "/declared/public/ping");
    let router = ok(router, "/declared/private");
    let server = WsTestServer::start_with_router(router).await;

    assert_eq!(server.get("/declared/public/ping").await.status, 200);
    assert_eq!(
        server.get("/declared/private").await.status,
        403,
        "a declared chain must govern its paths automatically"
    );
}

#[tokio::test]
async fn lower_order_wins_when_matchers_overlap() {
    let server = WsTestServer::start_with_router(ok(Router::new(), "/overlap/check")).await;

    assert_eq!(
        server.get("/overlap/check").await.status,
        200,
        "order = 5 must be evaluated before order = 100"
    );
}
