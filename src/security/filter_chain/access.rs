use crate::routing::engine::RequestContext;
use crate::routing::matcher::RequestMatcher;
use std::sync::Arc;

/// The result of evaluating one authorization rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessDecision {
    /// Continue to the handler.
    Grant,
    /// No authenticated identity: send the authentication entry point.
    Unauthenticated,
    /// Authenticated, but not permitted: send the access-denied handler.
    Deny,
}

/// One authorization rule: who may reach the requests this rule matches.
///
/// Rules are evaluated after all filters ran, so authentication filters have
/// already populated the session and/or JWT claims.
#[derive(Clone)]
pub enum AccessRule {
    /// Anyone, authenticated or not.
    PermitAll,
    /// Nobody. Useful as an explicit fallback while a route is being built.
    DenyAll,
    /// Any authenticated principal, regardless of role.
    Authenticated,
    /// A principal whose role satisfies the framework's RBAC hierarchy.
    HasRole(String),
    /// A principal satisfying at least one of the listed roles.
    HasAnyRole(Vec<String>),
    /// A principal satisfying every listed role.
    HasAllRoles(Vec<String>),
    /// Arbitrary logic. Return [`AccessDecision`] to control the response.
    Custom(Arc<dyn Fn(&RequestContext) -> AccessDecision + Send + Sync>),
}

impl AccessRule {
    pub fn permit_all() -> Self {
        Self::PermitAll
    }

    pub fn deny_all() -> Self {
        Self::DenyAll
    }

    pub fn authenticated() -> Self {
        Self::Authenticated
    }

    pub fn has_role(role: impl Into<String>) -> Self {
        Self::HasRole(role.into())
    }

    pub fn has_any_role<I, S>(roles: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::HasAnyRole(roles.into_iter().map(Into::into).collect())
    }

    pub fn has_all_roles<I, S>(roles: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::HasAllRoles(roles.into_iter().map(Into::into).collect())
    }

    pub fn custom<F>(predicate: F) -> Self
    where
        F: Fn(&RequestContext) -> AccessDecision + Send + Sync + 'static,
    {
        Self::Custom(Arc::new(predicate))
    }

    pub fn evaluate(&self, ctx: &RequestContext) -> AccessDecision {
        match self {
            AccessRule::PermitAll => AccessDecision::Grant,
            AccessRule::DenyAll => AccessDecision::Deny,
            AccessRule::Authenticated => {
                if is_authenticated(ctx) {
                    AccessDecision::Grant
                } else {
                    AccessDecision::Unauthenticated
                }
            }
            AccessRule::HasRole(role) => {
                if !is_authenticated(ctx) {
                    AccessDecision::Unauthenticated
                } else if ctx.has_role(role) {
                    AccessDecision::Grant
                } else {
                    AccessDecision::Deny
                }
            }
            AccessRule::HasAnyRole(roles) => {
                if !is_authenticated(ctx) {
                    AccessDecision::Unauthenticated
                } else if roles.iter().any(|role| ctx.has_role(role)) {
                    AccessDecision::Grant
                } else {
                    AccessDecision::Deny
                }
            }
            AccessRule::HasAllRoles(roles) => {
                if !is_authenticated(ctx) {
                    AccessDecision::Unauthenticated
                } else if roles.iter().all(|role| ctx.has_role(role)) {
                    AccessDecision::Grant
                } else {
                    AccessDecision::Deny
                }
            }
            AccessRule::Custom(predicate) => predicate(ctx),
        }
    }
}

/// A matcher paired with the rule that applies when it matches.
#[derive(Clone)]
pub struct AuthorizationRule {
    pub matcher: RequestMatcher,
    pub rule: AccessRule,
}

impl AuthorizationRule {
    pub fn new(matcher: impl Into<RequestMatcher>, rule: AccessRule) -> Self {
        Self {
            matcher: matcher.into(),
            rule,
        }
    }
}

/// True for either authentication mechanism the framework understands.
///
/// `RequestContext::is_user_authenticated` only inspects the session, which is
/// correct for cookie flows but would reject a valid bearer token. Authorization
/// rules need the union of both.
pub fn is_authenticated(ctx: &RequestContext) -> bool {
    if ctx.claims.is_some() {
        return true;
    }

    match &ctx.session {
        Some(session) => session
            .lock()
            .map(|session| session.user_id.is_some() || session.data.contains_key("user_id"))
            .unwrap_or(false),
        None => false,
    }
}
