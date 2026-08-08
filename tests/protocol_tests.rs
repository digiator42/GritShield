use gritshield::http::request::{HttpMethod, Request};
use gritshield::http::response::{Cookie, Response};
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
