use gritshield::http::request::{HttpMethod, Request};
use gritshield::http::response::{reason_phrase, Cookie, Response};
use gritshield::security::xss::Sanitizer;
use gritshield::security::xss::UntrustedString;
use std::collections::HashMap;

#[test]
fn test_request_builder_and_normalization() {
    let mut headers = HashMap::new();
    headers.insert("host".to_string(), vec!["127.0.0.1".to_string()]);
    headers.insert(
        "content-type".to_string(),
        vec!["application/x-www-form-urlencoded".to_string()],
    );

    let mut query = HashMap::new();
    query.insert(
        "debug".to_string(),
        vec![UntrustedString::new("true".to_string())],
    );

    let request = Request::fill(
        HttpMethod::POST,
        "/api/v1/secure-endpoint".to_string(),
        "http::/127.0.0.1:8080".to_string(),
        headers,
        b"username=admin&token=secret_token".to_vec(),
        query,
    );

    assert_eq!(request.method, HttpMethod::POST);
    assert_eq!(request.path, "/api/v1/secure-endpoint");
    assert_eq!(request.query.get("debug").unwrap().first().unwrap().as_str(), "true");
}

#[test]
fn test_response_status_and_header_emission() {
    let mut response = Response::ok(Sanitizer::trust("Execution safe"));
    response
        .headers
        .push(("X-Shield-Defended".to_string(), "True".to_string()));
    response = response.with_cookie(Cookie::new("session_token", "abc123secret"));

    assert_eq!(response.status, 200);
    assert!(response
        .headers
        .iter()
        .any(|(key, value)| { key == "X-Shield-Defended" && value == "True" }));
    assert_eq!(response.cookies.len(), 1);
}

// ==============================================================================
// MIDDLEWARE -> RESPONSE HEADER MERGE
// ==============================================================================

/// Builds the header map the server would have after a request arrives and
/// middleware annotates it, plus the set of names that came from the client.
fn middleware_header_scenario() -> (
    std::collections::HashMap<String, Vec<String>>,
    std::collections::HashSet<String>,
) {
    let request_headers = vec![
        ("cookie", "GSESSION_ID=secret123".to_string()),
        ("authorization", "Bearer topsecrettoken".to_string()),
        ("host", "127.0.0.1:8081".to_string()),
        ("user-agent", "curl/8.0".to_string()),
    ];

    // Request parsing lowercases every incoming name.
    let request_header_names: std::collections::HashSet<String> = request_headers
        .iter()
        .map(|(k, _)| k.to_lowercase())
        .collect();

    let mut ctx_headers: HashMap<String, Vec<String>> = request_headers
        .iter()
        .map(|(k, v)| (k.to_string(), vec![v.clone()]))
        .collect();

    // Middleware annotates the context, which is the only channel it has.
    ctx_headers.insert("x-request-id".to_string(), vec!["req-000042".to_string()]);

    (ctx_headers, request_header_names)
}

#[test]
fn test_client_headers_are_not_reflected_into_response() {
    let (ctx_headers, request_header_names) = middleware_header_scenario();
    let mut response = Response::ok(Sanitizer::trust("ok"));

    response.merge_middleware_headers(&ctx_headers, &request_header_names);

    for leaked in ["cookie", "authorization", "host", "user-agent"] {
        assert!(
            !response
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case(leaked)),
            "request header `{leaked}` must never be echoed into the response"
        );
    }
}

/// The reason the channel exists at all: middleware sets a response header.
#[test]
fn test_middleware_added_headers_still_reach_response() {
    let (ctx_headers, request_header_names) = middleware_header_scenario();
    let mut response = Response::ok(Sanitizer::trust("ok"));

    response.merge_middleware_headers(&ctx_headers, &request_header_names);

    let found: Vec<&(String, String)> = response
        .headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("x-request-id"))
        .collect();

    assert_eq!(
        found.len(),
        1,
        "middleware header should be carried forward exactly once"
    );
    assert_eq!(found[0].1, "req-000042");
}

/// Regression: the dedup check used `==`, so a handler's `X-Request-Id` and a
/// middleware's `x-request-id` were both sent, giving clients a doubled value.
#[test]
fn test_header_dedup_is_case_insensitive() {
    let (ctx_headers, request_header_names) = middleware_header_scenario();
    let mut response = Response::ok(Sanitizer::trust("ok"))
        .with_header("X-Request-Id", "req-000042");

    response.merge_middleware_headers(&ctx_headers, &request_header_names);

    let matching: Vec<&String> = response
        .headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("x-request-id"))
        .map(|(_, v)| v)
        .collect();

    assert_eq!(
        matching,
        vec!["req-000042"],
        "a header the handler already set must not be duplicated by casing alone"
    );
}

/// Multi-value middleware headers should all survive, not just the first.
#[test]
fn test_middleware_multi_value_headers_are_preserved() {
    let mut ctx_headers: HashMap<String, Vec<String>> = HashMap::new();
    ctx_headers.insert(
        "x-trace".to_string(),
        vec!["first".to_string(), "second".to_string()],
    );
    let request_header_names = std::collections::HashSet::new();

    let mut response = Response::ok(Sanitizer::trust("ok"));
    response.merge_middleware_headers(&ctx_headers, &request_header_names);

    let values: Vec<&String> = response
        .headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("x-trace"))
        .map(|(_, v)| v)
        .collect();

    assert_eq!(values, vec!["first", "second"]);
}

#[test]
fn test_method_not_allowed_status_is_available_to_catch_handlers() {
    use gritshield::http::response::HttpStatus;

    // A `#[catch(status = 405)]` handler has to build its response through
    // `Response::json`, which takes an `HttpStatus` rather than a bare number.
    let body = serde_json::json!({ "error": "wrong verb" });
    let response = Response::json(HttpStatus::MethodNotAllowed, &body)
        .with_header("Allow", "PUT, PATCH, DELETE");

    assert_eq!(response.status, 405);
    assert_eq!(HttpStatus::MethodNotAllowed.code(), 405);
    assert_eq!(reason_phrase(405), "Method Not Allowed");
    assert!(response
        .headers
        .iter()
        .any(|(k, v)| k == "Allow" && v == "PUT, PATCH, DELETE"));
    assert!(matches!(response.body, gritshield::http::response::ResponseBody::Json(_)));
}

// ==============================================================================
// HEADER NAME CASING
// ==============================================================================

/// `Request::parse` lowercases every incoming name; `Request::fill` keeps the
/// caller's casing. A `headers.get("Content-Type")` therefore matches live
/// traffic never and `fill`-built contexts only by accident.
#[test]
fn test_header_lookup_is_case_insensitive() {
    fn request_with(keys: &[&str]) -> Request {
        let mut headers = HashMap::new();
        for k in keys {
            headers.insert(k.to_string(), vec!["application/json".to_string()]);
        }
        Request::fill(
            HttpMethod::POST,
            "/xss/profile".to_string(),
            "http://127.0.0.1:8080".to_string(),
            headers,
            b"{}".to_vec(),
            HashMap::new(),
        )
    }

    // Lowercased, as `parse` produces it.
    let live = request_with(&["content-type"]);
    assert!(live.header("content-type").is_some());
    assert!(live.header("Content-Type").is_some());

    // Capitalised, as a hand-built `fill` in a test might use.
    let capitalised = request_with(&["Content-Type"]);
    assert!(
        capitalised.header("content-type").is_some(),
        "a capitalised Content-Type must still be found by the lowercase name"
    );

    // Mixed case, and absent.
    let mixed = request_with(&["CONTENT-type"]);
    assert!(mixed.header("content-type").is_some());
    assert!(request_with(&["accept"]).header("content-type").is_none());
}

/// Guards the specific regression: connection.rs read `headers.get("Content-Type")`,
/// which never matches a parsed request, so `ctx.content_type` was always `None`
/// and every `ctx.json::<T>()` call failed with "Content-Type must be
/// application/json" no matter what the client sent.
#[test]
fn test_parsed_request_content_type_is_reachable() {
    let mut headers = HashMap::new();
    // Exactly what `Request::parse` stores for a `Content-Type:` request line.
    headers.insert(
        "content-type".to_string(),
        vec!["application/json".to_string()],
    );

    let req = Request::fill(
        HttpMethod::POST,
        "/xss/profile".to_string(),
        "http://127.0.0.1:8080".to_string(),
        headers,
        br#"{"email":"a@b.c"}"#.to_vec(),
        HashMap::new(),
    );

    assert_eq!(
        req.header("content-type").and_then(|v| v.first()),
        Some(&"application/json".to_string()),
        "connection.rs builds ctx.content_type through this lookup"
    );
}
