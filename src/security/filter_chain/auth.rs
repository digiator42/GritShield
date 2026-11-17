use crate::core::env::get_env;
use crate::http::request::HttpMethod;
use crate::http::response::{Cookie, SameSite};
use crate::middleware::auth::get_session_store;
use crate::routing::engine::RequestContext;
use crate::routing::matcher::RequestMatcher;
use crate::security::filter_chain::{FilterResult, SecurityFilter};
use crate::security::jwt::JwtHandler;
use crate::security::session::{Session, SessionStore};
use sea_orm_migration::async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// Which credential source an [`AuthenticationFilter`] understands.
pub enum AuthMode {
    /// Cookie-backed sessions only.
    Session,
    /// Stateless bearer tokens only.
    Jwt(JwtHandler),
    /// Sessions first, bearer tokens when no authenticated session is present.
    Hybrid(JwtHandler),
}

/// Loads identity into the request without deciding whether it is enough.
///
/// This mirrors Spring Security's separation: the filter populates the security
/// context, and the chain's authorization rules make the access decision. An
/// invalid bearer token is the one exception — it returns
/// [`FilterResult::Unauthenticated`], so a tampered token never slides through a
/// `permitAll()` rule.
pub struct AuthenticationFilter {
    mode: AuthMode,
    store: Arc<SessionStore>,
    create_anonymous_sessions: bool,
    session_cookie: String,
}

impl AuthenticationFilter {
    /// Session authentication against the framework's shared session store.
    pub fn session() -> Self {
        Self::session_with_store(get_session_store().clone())
    }

    /// Session authentication against a caller-provided store.
    pub fn session_with_store(store: Arc<SessionStore>) -> Self {
        Self {
            mode: AuthMode::Session,
            store,
            create_anonymous_sessions: true,
            session_cookie: "GSESSION_ID".to_string(),
        }
    }

    /// Stateless JWT bearer authentication.
    pub fn jwt(handler: JwtHandler) -> Self {
        Self {
            mode: AuthMode::Jwt(handler),
            store: get_session_store().clone(),
            create_anonymous_sessions: false,
            session_cookie: "GSESSION_ID".to_string(),
        }
    }

    /// Sessions for browsers, bearer tokens for API clients.
    pub fn hybrid(handler: JwtHandler) -> Self {
        Self {
            mode: AuthMode::Hybrid(handler),
            store: get_session_store().clone(),
            create_anonymous_sessions: true,
            session_cookie: "GSESSION_ID".to_string(),
        }
    }

    /// Whether anonymous requests in session mode receive a session cookie.
    ///
    /// On by default, because a session is also where the CSRF token lives and
    /// server-rendered pages need one before the user logs in. Set to `false`
    /// for a chain that only creates sessions after a successful login.
    pub fn create_anonymous_sessions(mut self, create: bool) -> Self {
        self.create_anonymous_sessions = create;
        self
    }

    /// Override the signed session cookie name (`GSESSION_ID` by default).
    pub fn session_cookie_name(mut self, name: impl Into<String>) -> Self {
        self.session_cookie = name.into();
        self
    }

    fn load_session(&self, ctx: &mut RequestContext) -> bool {
        let cookie_session = ctx.get_signed_cookie(&self.session_cookie);

        let mut session = cookie_session.as_ref().and_then(|sid| {
            self.store
                .sessions
                .get(sid)
                .map(|entry| entry.value().clone())
        });

        if session.is_none() && cookie_session.is_some() {
            // The cookie survived but the server-side session did not: clear it
            // so the browser stops replaying a dead identifier.
            ctx.remove_cookie(&self.session_cookie);
        }

        if session.is_none() && self.create_anonymous_sessions {
            session = Some(self.mint_session(ctx));
        }

        let Some(session) = session else {
            return false;
        };

        let authenticated = match session.lock() {
            Ok(mut guard) => {
                guard.last_accessed = std::time::Instant::now();
                guard
                    .data
                    .entry("csrf_token".to_string())
                    .or_insert_with(|| uuid::Uuid::new_v4().to_string());
                guard.user_id.is_some() || guard.data.contains_key("user_id")
            }
            Err(_) => false,
        };

        ctx.session = Some(session);
        authenticated
    }

    fn mint_session(&self, ctx: &mut RequestContext) -> Arc<Mutex<Session>> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let session = Arc::new(Mutex::new(Session {
            id: session_id.clone(),
            data: std::collections::HashMap::new(),
            user_id: None,
            last_accessed: std::time::Instant::now(),
        }));

        self.store
            .sessions
            .insert(session_id.clone(), Arc::clone(&session));

        let is_production = get_env("APP_ENV", "development") == "production";
        let cookie = Cookie::new(&self.session_cookie, &session_id)
            .set_secure(is_production)
            .set_same_site(SameSite::Lax);

        ctx.set_signed_cookie(cookie);
        session
    }

    fn verify_bearer(&self, ctx: &mut RequestContext, handler: &JwtHandler) -> FilterResult {
        let Some(header) = ctx.header("authorization") else {
            return FilterResult::Continue;
        };

        let Some((scheme, token)) = header.split_once(' ') else {
            return FilterResult::Continue;
        };

        if !scheme.eq_ignore_ascii_case("bearer") {
            return FilterResult::Continue;
        }

        let token = token.trim();
        if token.is_empty() {
            return FilterResult::Unauthenticated;
        }

        match handler.verify(token) {
            Ok(claims) => {
                ctx.claims = Some(claims);
                FilterResult::Continue
            }
            Err(_) => FilterResult::Unauthenticated,
        }
    }
}

#[async_trait]
impl SecurityFilter for AuthenticationFilter {
    fn name(&self) -> &'static str {
        "authentication"
    }

    async fn on_request(&self, ctx: &mut RequestContext) -> FilterResult {
        match &self.mode {
            AuthMode::Session => {
                self.load_session(ctx);
                FilterResult::Continue
            }
            AuthMode::Jwt(handler) => self.verify_bearer(ctx, handler),
            AuthMode::Hybrid(handler) => {
                if self.load_session(ctx) {
                    // An authenticated session wins; a stale bearer token next
                    // to it must not downgrade the request.
                    FilterResult::Continue
                } else {
                    self.verify_bearer(ctx, handler)
                }
            }
        }
    }
}

/// Rejects state-changing requests whose anti-forgery token does not match the
/// token stored in the session.
///
/// Only meaningful in a session (or hybrid) chain. Add it after
/// [`AuthenticationFilter::session`] so the session and its CSRF token exist.
/// Public form endpoints such as login can opt out with [`Self::ignore`].
pub struct CsrfFilter {
    header_name: String,
    field_name: String,
    methods: Vec<HttpMethod>,
    ignored: Vec<RequestMatcher>,
}

impl Default for CsrfFilter {
    fn default() -> Self {
        Self::new()
    }
}

impl CsrfFilter {
    pub fn new() -> Self {
        Self {
            header_name: "x-csrf-token".to_string(),
            field_name: "csrf_token".to_string(),
            methods: vec![
                HttpMethod::POST,
                HttpMethod::PUT,
                HttpMethod::PATCH,
                HttpMethod::DELETE,
            ],
            ignored: Vec::new(),
        }
    }

    /// Read the token from a different request header.
    pub fn header_name(mut self, name: impl Into<String>) -> Self {
        self.header_name = name.into().to_ascii_lowercase();
        self
    }

    /// Read the token from a different form field.
    pub fn field_name(mut self, name: impl Into<String>) -> Self {
        self.field_name = name.into();
        self
    }

    /// Skip the check for requests matching `matcher` (for example the login
    /// POST that starts the session).
    pub fn ignore(mut self, matcher: impl Into<RequestMatcher>) -> Self {
        self.ignored.push(matcher.into());
        self
    }

    /// Skip the check for a path pattern.
    pub fn ignore_path(mut self, pattern: impl Into<String>) -> Self {
        self.ignored.push(RequestMatcher::path(pattern));
        self
    }

    fn tokens_match(expected: &str, supplied: &str) -> bool {
        let (expected, supplied) = (expected.as_bytes(), supplied.as_bytes());
        if expected.len() != supplied.len() {
            return false;
        }

        let mut difference = 0u8;
        for (a, b) in expected.iter().zip(supplied.iter()) {
            difference |= a ^ b;
        }
        difference == 0
    }
}

#[async_trait]
impl SecurityFilter for CsrfFilter {
    fn name(&self) -> &'static str {
        "csrf"
    }

    async fn on_request(&self, ctx: &mut RequestContext) -> FilterResult {
        if !self.methods.contains(&ctx.req.method) {
            return FilterResult::Continue;
        }

        if self.ignored.iter().any(|matcher| matcher.matches(ctx)) {
            return FilterResult::Continue;
        }

        let expected = ctx.get_csrf_token();
        if expected.is_empty() {
            // No session means no anti-forgery token can be validated. Fail
            // closed rather than silently waving the request through.
            return FilterResult::AccessDenied;
        }

        let supplied = ctx
            .header(&self.header_name)
            .map(str::to_string)
            .or_else(|| ctx.form.get_plain_str(&self.field_name).map(str::to_string));

        match supplied {
            Some(token) if Self::tokens_match(&expected, &token) => FilterResult::Continue,
            _ => FilterResult::AccessDenied,
        }
    }
}
