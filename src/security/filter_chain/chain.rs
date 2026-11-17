use crate::http::response::Response;
use crate::routing::engine::RequestContext;
use crate::routing::matcher::RequestMatcher;
use crate::security::filter_chain::{
    AccessDecision, AccessDeniedHandler, AccessRule, AuthenticationEntryPoint,
    AuthenticationFilter, AuthorizationRule, CsrfFilter, FilterResult, SecurityFilter,
};
use crate::security::jwt::JwtHandler;

/// What happened when the router offered a request to its security chains.
pub enum SecurityChainResult {
    /// No chain claimed the request; the request continues unchanged.
    NoChain,
    /// A chain claimed the request and allowed it through.
    Continue { chain: usize, filters: usize },
    /// A chain claimed the request and produced this response.
    Respond {
        chain: usize,
        filters: usize,
        response: Response,
    },
}

pub(crate) enum ChainOutcome {
    Continue { filters: usize },
    Respond { filters: usize, response: Response },
}

/// An ordered group of filters plus authorization rules for the requests its
/// matcher covers.
///
/// Chains are evaluated in registration order and the first matching chain wins,
/// exactly like Spring Security's `SecurityFilterChain`. A request matched by no
/// chain is untouched, so chains are opt-in and do not disturb existing
/// middleware-based applications.
pub struct SecurityFilterChain {
    name: String,
    matcher: RequestMatcher,
    filters: Vec<Box<dyn SecurityFilter>>,
    rules: Vec<AuthorizationRule>,
    any_request: Option<AccessRule>,
    entry_point: AuthenticationEntryPoint,
    access_denied: AccessDeniedHandler,
}

impl std::fmt::Debug for SecurityFilterChain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecurityFilterChain")
            .field("name", &self.name)
            .field("matcher", &self.matcher)
            .field("filters", &self.filters.len())
            .field("rules", &self.rules.len())
            .field("has_any_request", &self.any_request.is_some())
            .finish()
    }
}

impl SecurityFilterChain {
    pub fn builder() -> SecurityFilterChainBuilder {
        SecurityFilterChainBuilder::new()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn matches(&self, ctx: &RequestContext) -> bool {
        self.matcher.matches(ctx)
    }

    fn access_decision(&self, ctx: &RequestContext) -> AccessDecision {
        let matched_rule = self
            .rules
            .iter()
            .find(|rule| rule.matcher.matches(ctx))
            .map(|rule| &rule.rule)
            .or(self.any_request.as_ref());

        match matched_rule {
            Some(rule) => rule.evaluate(ctx),
            // A chain with rules is fail-closed: anything the rules did not
            // cover is denied. A chain with no rules at all is filters-only and
            // deliberately leaves authorization to the handler.
            None => {
                if self.rules.is_empty() {
                    AccessDecision::Grant
                } else {
                    AccessDecision::Deny
                }
            }
        }
    }

    pub(crate) async fn run(&self, ctx: &mut RequestContext) -> ChainOutcome {
        let mut executed = 0;

        for filter in &self.filters {
            executed += 1;

            match filter.on_request(ctx).await {
                FilterResult::Continue => {}
                FilterResult::Respond(response) => {
                    return ChainOutcome::Respond {
                        filters: executed,
                        response,
                    }
                }
                FilterResult::Unauthenticated => {
                    return ChainOutcome::Respond {
                        filters: executed,
                        response: self.entry_point.respond(ctx),
                    }
                }
                FilterResult::AccessDenied => {
                    return ChainOutcome::Respond {
                        filters: executed,
                        response: self.access_denied.respond(ctx),
                    }
                }
            }
        }

        match self.access_decision(ctx) {
            AccessDecision::Grant => ChainOutcome::Continue { filters: executed },
            AccessDecision::Unauthenticated => ChainOutcome::Respond {
                filters: executed,
                response: self.entry_point.respond(ctx),
            },
            AccessDecision::Deny => ChainOutcome::Respond {
                filters: executed,
                response: self.access_denied.respond(ctx),
            },
        }
    }

    pub(crate) async fn run_on_response(
        &self,
        ctx: &RequestContext,
        res: &mut Response,
        executed: usize,
    ) {
        for filter in self.filters.iter().take(executed).rev() {
            filter.on_response(ctx, res).await;
        }
    }
}

/// Fluent builder for a [`SecurityFilterChain`].
///
/// ```rust,no_run,ignore
/// use gritshield::prelude::*;
///
/// let chain = SecurityFilterChain::builder()
///     .name("api")
///     .matching("/api/**")
///     .jwt_auth(JwtHandler::new(&secret))
///     .authorize_public("/api/public/**")
///     .authorize_role("Admin", "/api/admin/**")
///     .any_request(AccessRule::authenticated())
///     .entry_point(AuthenticationEntryPoint::bearer())
///     .build();
///
/// let router = Router::new().add_security_chain(chain);
/// ```
pub struct SecurityFilterChainBuilder {
    name: String,
    matcher: RequestMatcher,
    filters: Vec<Box<dyn SecurityFilter>>,
    rules: Vec<AuthorizationRule>,
    any_request: Option<AccessRule>,
    entry_point: AuthenticationEntryPoint,
    access_denied: AccessDeniedHandler,
}

impl Default for SecurityFilterChainBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityFilterChainBuilder {
    pub fn new() -> Self {
        Self {
            name: "security".to_string(),
            matcher: RequestMatcher::any(),
            filters: Vec::new(),
            rules: Vec::new(),
            any_request: None,
            entry_point: AuthenticationEntryPoint::default(),
            access_denied: AccessDeniedHandler::default(),
        }
    }

    /// A label used in diagnostics.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Only govern requests whose path matches `pattern`.
    pub fn matching(mut self, pattern: impl Into<String>) -> Self {
        self.matcher = RequestMatcher::path(pattern);
        self
    }

    /// Govern requests selected by an arbitrary matcher (path, method, header,
    /// or predicate).
    pub fn matching_request(mut self, matcher: impl Into<RequestMatcher>) -> Self {
        self.matcher = matcher.into();
        self
    }

    /// Append a filter. Existing middlewares work here through the blanket
    /// `SecurityFilter` implementation.
    pub fn filter<F>(mut self, filter: F) -> Self
    where
        F: SecurityFilter + 'static,
    {
        self.filters.push(Box::new(filter));
        self
    }

    /// Add cookie-session authentication (`GSESSION_ID`, signed).
    pub fn session_auth(self) -> Self {
        self.filter(AuthenticationFilter::session())
    }

    /// Add bearer-token authentication.
    pub fn jwt_auth(self, handler: JwtHandler) -> Self {
        self.filter(AuthenticationFilter::jwt(handler))
    }

    /// Add session authentication with a bearer-token fallback.
    pub fn hybrid_auth(self, handler: JwtHandler) -> Self {
        self.filter(AuthenticationFilter::hybrid(handler))
    }

    /// Add the anti-forgery guard for state-changing methods.
    pub fn csrf(self) -> Self {
        self.filter(CsrfFilter::new())
    }

    /// Add the anti-forgery guard, configured by the closure.
    pub fn csrf_with<F>(self, configure: F) -> Self
    where
        F: FnOnce(CsrfFilter) -> CsrfFilter,
    {
        self.filter(configure(CsrfFilter::new()))
    }

    /// Authorize requests matching `matcher` with `rule`.
    ///
    /// Rules are checked in the order they are added; the first matching rule
    /// wins.
    pub fn authorize(mut self, matcher: impl Into<RequestMatcher>, rule: AccessRule) -> Self {
        self.rules.push(AuthorizationRule::new(matcher, rule));
        self
    }

    /// Authorize requests matching a path pattern with `rule`.
    pub fn authorize_pattern(mut self, pattern: impl Into<String>, rule: AccessRule) -> Self {
        self.rules
            .push(AuthorizationRule::new(RequestMatcher::path(pattern), rule));
        self
    }

    /// Shorthand for `authorize_pattern(pattern, AccessRule::permit_all())`.
    pub fn authorize_public(self, pattern: impl Into<String>) -> Self {
        self.authorize_pattern(pattern, AccessRule::permit_all())
    }

    /// Shorthand for `authorize_pattern(pattern, AccessRule::authenticated())`.
    pub fn authorize_authenticated(self, pattern: impl Into<String>) -> Self {
        self.authorize_pattern(pattern, AccessRule::authenticated())
    }

    /// Shorthand for a role-protected path.
    pub fn authorize_role(self, role: impl Into<String>, pattern: impl Into<String>) -> Self {
        self.authorize_pattern(pattern, AccessRule::has_role(role))
    }

    /// The rule applied when no earlier rule matched.
    ///
    /// A chain with no rules at all is filters-only and permits the request
    /// after its filters run. As soon as at least one rule exists, unmatched
    /// requests are denied — secure by default.
    pub fn any_request(mut self, rule: AccessRule) -> Self {
        self.any_request = Some(rule);
        self
    }

    /// The response for a request with no usable identity.
    pub fn entry_point(mut self, entry_point: AuthenticationEntryPoint) -> Self {
        self.entry_point = entry_point;
        self
    }

    /// The response for an identified principal that is not permitted.
    pub fn access_denied(mut self, handler: AccessDeniedHandler) -> Self {
        self.access_denied = handler;
        self
    }

    pub fn build(self) -> SecurityFilterChain {
        SecurityFilterChain {
            name: self.name,
            matcher: self.matcher,
            filters: self.filters,
            rules: self.rules,
            any_request: self.any_request,
            entry_point: self.entry_point,
            access_denied: self.access_denied,
        }
    }
}
