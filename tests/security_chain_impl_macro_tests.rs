//! `#[security_chain]` on an inherent impl block: the chain is built from the
//! type's associated function and auto-applied like the function form.

use gritshield::http::HttpMethod;
use gritshield::prelude::*;
use gritshield::testing::WsTestServer;

struct DeclaredImplChain;

#[security_chain(name = "impl-declared", order = 10)]
impl DeclaredImplChain {
    fn build() -> SecurityFilterChain {
        SecurityFilterChain::builder()
            .matching("/impl/**")
            .authorize_public("/impl/open")
            .build()
    }
}

fn ok(router: Router, path: &str) -> Router {
    router.route((path, HttpMethod::GET, |_ctx: RequestContext| async {
        Response::ok(Sanitizer::trust("ok"))
    }))
}

#[test]
fn impl_chain_is_registered_under_its_name() {
    let router = Router::new();
    let chain = router
        .security_chains
        .iter()
        .find(|chain| chain.name() == "impl-declared")
        .expect("declared impl chain is auto-collected");

    assert_eq!(chain.name(), "impl-declared");
}

#[tokio::test]
async fn impl_chain_governs_its_paths() {
    let router = ok(Router::new(), "/impl/open");
    let router = ok(router, "/impl/closed");
    let server = WsTestServer::start_with_router(router).await;

    assert_eq!(server.get("/impl/open").await.status, 200);
    assert_eq!(
        server.get("/impl/closed").await.status,
        403,
        "paths not covered by a rule fail closed"
    );
}
