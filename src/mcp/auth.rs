use crate::core::env::get_env;
use crate::routing::engine::RequestContext;
use crate::security::jwt::JwtHandler;
use serde::Serialize;

/// The identity resolved for an MCP request.
#[derive(Debug, Clone, Serialize)]
pub struct McpIdentity {
    pub subject: String,
    pub role: Option<String>,
    /// How the identity was established, for the audit trail.
    pub source: IdentitySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentitySource {
    /// A cryptographically verified `Authorization: Bearer` JWT.
    BearerToken,
    /// An authenticated framework session cookie.
    SessionCookie,
    /// No credentials were presented.
    Anonymous,
}

impl McpIdentity {
    pub fn anonymous() -> Self {
        Self {
            subject: "anonymous".to_string(),
            role: None,
            source: IdentitySource::Anonymous,
        }
    }

    pub fn is_authenticated(&self) -> bool {
        self.source != IdentitySource::Anonymous
    }
}

/// The verdict of the MCP authentication guard.
#[derive(Debug, Clone)]
pub enum McpAuthOutcome {
    Allowed(McpIdentity),
    Rejected { status: u16, reason: String },
}

impl McpAuthOutcome {
    pub fn is_allowed(&self) -> bool {
        matches!(self, McpAuthOutcome::Allowed(_))
    }

    pub fn identity(&self) -> Option<&McpIdentity> {
        match self {
            McpAuthOutcome::Allowed(identity) => Some(identity),
            McpAuthOutcome::Rejected { .. } => None,
        }
    }

    /// The audit label for this outcome.
    pub fn audit_label(&self) -> &'static str {
        match self {
            McpAuthOutcome::Allowed(identity) => match identity.source {
                IdentitySource::BearerToken => "bearer",
                IdentitySource::SessionCookie => "session",
                IdentitySource::Anonymous => "anonymous",
            },
            McpAuthOutcome::Rejected { .. } => "rejected",
        }
    }
}

/// The secret used to verify bearer tokens.
///
/// Mirrors `Router::new`, which also derives its signing secret from
/// `JWT_SECRET` (falling back to a per-boot random value). Sharing the variable
/// means one token works across the HTTP API and MCP without extra plumbing.
fn signing_secret() -> String {
    get_env("JWT_SECRET", "")
}

/// Whether anonymous MCP clients are refused outright.
///
/// Defaults to permissive so the server is usable out of the box, but every
/// tool still enforces its own `required_role`, so an anonymous caller can
/// only ever reach tools explicitly marked unrestricted. Set `MCP_REQUIRE_AUTH`
/// to close the anonymous path entirely.
pub fn require_authentication() -> bool {
    matches!(
        get_env("MCP_REQUIRE_AUTH", "false").to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

/// Resolve the caller's identity, validating any presented credentials.
///
/// A *malformed or unverifiable* bearer token is always a hard rejection. It is
/// never treated as "anonymous" — otherwise a client holding a stale token would
/// silently downgrade instead of being told to refresh it.
pub fn authenticate(ctx: &RequestContext) -> McpAuthOutcome {
    if let Some(header) = ctx.header("authorization") {
        if !header.trim().is_empty() {
            return match verify_authorization(header.trim(), &signing_secret()) {
                Ok(identity) => McpAuthOutcome::Allowed(identity),
                Err((status, reason)) => McpAuthOutcome::Rejected { status, reason },
            };
        }
    }

    // Fall back to the framework's own session state, which the auth middleware
    // has already validated and hydrated.
    if let Some(subject) = ctx.get_user_id() {
        return McpAuthOutcome::Allowed(McpIdentity {
            subject,
            role: ctx.get_user_role(),
            source: IdentitySource::SessionCookie,
        });
    }

    if require_authentication() {
        return McpAuthOutcome::Rejected {
            status: 401,
            reason: "MCP requires authentication: supply a Bearer token or an authenticated session"
                .to_string(),
        };
    }

    McpAuthOutcome::Allowed(McpIdentity::anonymous())
}

/// Verify an `Authorization` header value against a signing secret.
///
/// Split out from [`authenticate`] so the credential-handling rules can be
/// tested without mutating process-wide environment state.
pub fn verify_authorization(
    header: &str,
    secret: &str,
) -> Result<McpIdentity, (u16, String)> {
    let token = header
        .strip_prefix("Bearer ")
        .or_else(|| header.strip_prefix("bearer "))
        .map(str::trim)
        .ok_or_else(|| {
            (
                401,
                "Authorization header must use the 'Bearer <token>' scheme".to_string(),
            )
        })?;

    if token.is_empty() {
        return Err((401, "Bearer token was empty".to_string()));
    }

    if secret.is_empty() {
        return Err((
            500,
            "Bearer authentication is unavailable: JWT_SECRET is not configured".to_string(),
        ));
    }

    match JwtHandler::new(secret).verify(token) {
        Ok(claims) => Ok(McpIdentity {
            subject: claims.sub,
            role: Some(claims.role),
            source: IdentitySource::BearerToken,
        }),
        Err(reason) => Err((401, format!("Bearer token rejected: {}", reason))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::jwt::Claims;

    const SECRET: &str = "gritshield-mcp-unit-test-secret";

    #[test]
    fn a_non_bearer_authorization_scheme_is_rejected() {
        let (status, reason) =
            verify_authorization("Basic dXNlcjpwYXNz", SECRET).unwrap_err();
        assert_eq!(status, 401);
        assert!(reason.contains("Bearer"));
    }

    #[test]
    fn a_forged_bearer_token_is_rejected_not_downgraded() {
        let (status, reason) =
            verify_authorization("Bearer not.a.jwt", SECRET).unwrap_err();
        assert_eq!(status, 401);
        assert!(reason.contains("rejected"));
    }

    #[test]
    fn a_valid_bearer_token_resolves_subject_and_role() {
        let claims = Claims::new("agent-42".to_string(), "Admin".to_string(), 600);
        let token = JwtHandler::new(SECRET).sign(&claims).unwrap();

        let identity = verify_authorization(&format!("Bearer {}", token), SECRET).unwrap();
        assert_eq!(identity.subject, "agent-42");
        assert_eq!(identity.role.as_deref(), Some("Admin"));
        assert_eq!(identity.source, IdentitySource::BearerToken);
        assert!(identity.is_authenticated());
    }

    #[test]
    fn a_token_signed_with_another_secret_is_rejected() {
        let claims = Claims::new("mallory".to_string(), "SuperAdmin".to_string(), 600);
        let token = JwtHandler::new("a-different-secret").sign(&claims).unwrap();

        let (status, _) = verify_authorization(&format!("Bearer {}", token), SECRET).unwrap_err();
        assert_eq!(status, 401, "signature must be verified, not merely parsed");
    }

    #[test]
    fn an_empty_token_is_rejected() {
        assert_eq!(verify_authorization("Bearer ", SECRET).unwrap_err().0, 401);
    }

    #[test]
    fn bearer_auth_without_a_configured_secret_fails_loudly() {
        let (status, reason) = verify_authorization("Bearer a.b.c", "").unwrap_err();
        assert_eq!(status, 500);
        assert!(reason.contains("JWT_SECRET"));
    }

    #[test]
    fn anonymous_is_allowed_when_auth_is_optional() {
        let outcome = authenticate(&RequestContext::new());
        assert!(outcome.is_allowed());
        let identity = outcome.identity().unwrap();
        assert_eq!(identity.source, IdentitySource::Anonymous);
        assert!(!identity.is_authenticated());
    }

    #[test]
    fn a_bad_bearer_header_on_a_context_is_rejected() {
        let mut ctx = RequestContext::new();
        ctx.headers.insert(
            "authorization".to_string(),
            vec!["Basic abc".to_string()],
        );

        let outcome = authenticate(&ctx);
        assert!(!outcome.is_allowed());
        assert_eq!(outcome.audit_label(), "rejected");
        assert!(outcome.identity().is_none());
    }

    #[test]
    fn empty_authorization_header_falls_through_to_anonymous() {
        let mut ctx = RequestContext::new();
        ctx.headers.insert("authorization".to_string(), vec![String::new()]);

        assert!(authenticate(&ctx).is_allowed());
    }
}