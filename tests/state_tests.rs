use gritshield::http::request::{HttpMethod, Request};
use gritshield::routing::engine::RequestContext;
use gritshield::security::cookies::CookieJar;
use gritshield::security::session::{Session, SessionStore};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

// ==============================================================================
// TEST 1: COOKIE PARSING AND JAR MANAGEMENT
// ==============================================================================
#[test]
fn test_cookie_jar_parsing_and_extraction() {
    // Simulate an incoming Cookie header from a client browser
    let mut headers = HashMap::new();
    let cookie_header_value =
        "GSESSION_ID=grit_xyz123abc; theme=dark; Secure; HttpOnly".to_string();
    headers.insert("cookie".to_string(), cookie_header_value.clone());

    // Parse the request headers via your framework's CookieJar implementation
    let jar = CookieJar::new(Some(&cookie_header_value), "None".to_string());

    // Verify values match original states perfectly
    let session_cookie = jar.get("GSESSION_ID");

    assert!(
        session_cookie.is_some(),
        "CookieJar failed to extract 'GSESSION_ID'"
    );
    assert_eq!(session_cookie.unwrap().as_str(), "grit_xyz123abc");

    let theme_cookie = jar.get("theme");
    assert!(theme_cookie.is_some());
    assert_eq!(theme_cookie.unwrap().as_str(), "dark");
}

// ==============================================================================
// TEST 2: SESSION LIFECYCLE MANAGEMENT
// ==============================================================================
#[test]
fn test_session_store_crud_lifecycle() {
    let store = SessionStore::new();

    // Unknown ID -> store creates a session under its OWN generated id
    let (session, created) = store.get_or_create(Some("test_user_session_uuid_999".to_string()));
    assert!(created, "first call should create a new session");

    // The real storage key is the generated id, not what we passed
    let real_id = session.lock().unwrap().id.clone();

    {
        let mut guard = session.lock().unwrap();
        guard
            .data
            .insert("user_id".to_string(), "admin_442".to_string());
        guard
            .data
            .insert("is_authenticated".to_string(), "true".to_string());
    }

    // Resume using the REAL id
    let (retrieved, created_again) = store.get_or_create(Some(real_id));
    assert!(
        !created_again,
        "SessionStore failed to resolve saved session"
    );

    let lock_guard = retrieved.lock().unwrap();

    assert_eq!(lock_guard.data.get("user_id").unwrap(), "admin_442");
    assert_eq!(lock_guard.data.get("is_authenticated").unwrap(), "true");
}
