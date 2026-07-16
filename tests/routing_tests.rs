use gritshield::futures::future::FutureExt;
use gritshield::http::request::{HttpMethod, Request};
use gritshield::http::response::Response;
use gritshield::routing::trie::{RequestContext, Router, RoutingResult};
use gritshield::security::xss::Sanitizer;
use std::collections::HashMap;

// Define a Mock Handler for a route
async fn handle_checkout(_ctx: RequestContext) -> Response {
    Response::ok(Sanitizer::trust("Checkout Complete"))
}

// ==============================================================================
// TEST 1: CORE ROUTE MATCHING AND CLOSURE DISPATCHING
// ==============================================================================
#[test]
fn test_router_registration_and_matching() {
    let router = Router::new().route((
        "/api/v1/checkout",
        HttpMethod::POST,
        move |ctx: RequestContext| async move { handle_checkout(ctx).await }.boxed(),
    ));

    // Simulate an incoming raw request context
    let request = Request::fill(
        HttpMethod::POST,
        "/api/v1/checkout".to_string(),
        "http://127.0.0.1:8080".to_string(),
        HashMap::new(),
        vec![],
        HashMap::new(),
    );

    let mut ctx = RequestContext::new();
    ctx.req = request;

    // Look up the handler in your trie
    let matched_route = router.match_route(&HttpMethod::POST, "/api/v1/checkout");

    // Assert using structural match instead of .is_some()
    assert!(
        matches!(matched_route, RoutingResult::Found(_, _, _)),
        "Router failed to match explicit static path"
    );
}

// ==============================================================================
// TEST 2: CORE TRIE ROUTING MATCH & DISPATCH EXTRACTION WITH DYNAMIC PARAMS
// ==============================================================================
#[test]
fn test_router_registration_matching_and_params() {
    // Instantiate a clean router state with a dynamic parameter segment (e.g., :id)
    let router = Router::new().route((
        "/api/v1/orders/:id/checkout",
        HttpMethod::POST,
        move |_ctx: RequestContext| {
            async move { Response::ok(Sanitizer::trust("Checkout Complete")) }.boxed()
        },
    ));

    // Build the incoming Request signatures
    let method = HttpMethod::POST;
    let path = "/api/v1/orders/ORD-99211/checkout".to_string();

    // Execute match_route using references exactly as your router expects
    let match_result = router.match_route(&method, &path);

    // Assert and unpack the structural variants
    match match_result {
        RoutingResult::Found(handler, required_role, dynamic_params) => {
            // Verify dynamic URL parameters were correctly parsed out of the path by your Trie
            assert!(
                dynamic_params.contains_key("id"),
                "Trie matching failed to parse the dynamic placeholder parameter ':id'"
            );
            assert_eq!(dynamic_params.get("id").unwrap().to_string(), "ORD-99211");

            // Verify role extraction if any
            assert!(required_role.is_none());

            // Build context and execute the handler reference to verify call stability
            let request = Request::fill(
                method,
                path,
                "http://127.0.0.1:8080".to_string(),
                HashMap::new(),
                vec![],
                dynamic_params, // Pass parsed parameters forward
            );

            let mut ctx = RequestContext::new();
            ctx.req = request;

            // Execute the boxed future out of your trait object handler
            let handler_fut = handler.call(ctx);

            let response = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(handler_fut);

            assert_eq!(response.status, 200);
        }
        RoutingResult::MethodNotAllowed => {
            panic!("Router matching returned unexpected MethodNotAllowed variant")
        }
        RoutingResult::NotFound => {
            panic!("Router matching failed to locate the registered dynamic path")
        }
    }
}
