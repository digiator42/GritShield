use crate::http::response::Response;
use crate::routing::engine::RequestContext;
use crate::security::xss::Sanitizer;
use std::sync::Arc;

type EntryPointFn = Arc<dyn Fn(&RequestContext) -> Response + Send + Sync>;
type DeniedFn = Arc<dyn Fn(&RequestContext) -> Response + Send + Sync>;

/// Responds when authorization determines that no usable identity is present.
///
/// This is the place to send a browser to `/login` while an API client gets a
/// `401`. The default is a `401` JSON body.
#[derive(Clone, Default)]
pub enum AuthenticationEntryPoint {
    /// `401` with a JSON `{"error": "Authentication required"}` body.
    #[default]
    Unauthorized,
    /// `401` JSON plus `WWW-Authenticate: Bearer`, for token-protected APIs.
    Bearer,
    /// `401` HTML, matching `AuthMiddleware`'s traditional response.
    UnauthorizedHtml,
    /// `303 See Other` to a login page. `AuthMiddleware`-compatible default
    /// for server-rendered applications.
    Redirect(String),
    /// Any response you want.
    Custom(EntryPointFn),
}

impl AuthenticationEntryPoint {
    pub fn redirect(location: impl Into<String>) -> Self {
        Self::Redirect(location.into())
    }

    pub fn custom<F>(handler: F) -> Self
    where
        F: Fn(&RequestContext) -> Response + Send + Sync + 'static,
    {
        Self::Custom(Arc::new(handler))
    }

    pub fn respond(&self, ctx: &RequestContext) -> Response {
        match self {
            AuthenticationEntryPoint::Unauthorized => {
                Response::json_unauthorized_msg("Authentication required")
            }
            AuthenticationEntryPoint::Bearer => {
                Response::json_unauthorized_msg("Authentication required")
                    .with_header("WWW-Authenticate", "Bearer")
            }
            AuthenticationEntryPoint::UnauthorizedHtml => Response::new(
                401,
                Sanitizer::trust("<h1>401 Unauthorized</h1><p>Access Denied.</p>"),
            ),
            AuthenticationEntryPoint::Redirect(location) => Response::redirect(303, location),
            AuthenticationEntryPoint::Custom(handler) => handler(ctx),
        }
    }
}

/// Responds when an identified principal is not permitted to continue.
///
/// The default is a `403` JSON body. Use [`Self::redirect`] to bounce
/// authenticated-but-unverified users somewhere friendlier, or
/// [`Self::custom`] for content negotiation.
#[derive(Clone, Default)]
pub enum AccessDeniedHandler {
    /// `403` with a JSON `{"error": "Access denied"}` body.
    #[default]
    Forbidden,
    /// `403` HTML.
    ForbiddenHtml,
    /// `303 See Other`.
    Redirect(String),
    Custom(DeniedFn),
}

impl AccessDeniedHandler {
    pub fn redirect(location: impl Into<String>) -> Self {
        Self::Redirect(location.into())
    }

    pub fn custom<F>(handler: F) -> Self
    where
        F: Fn(&RequestContext) -> Response + Send + Sync + 'static,
    {
        Self::Custom(Arc::new(handler))
    }

    pub fn respond(&self, ctx: &RequestContext) -> Response {
        match self {
            AccessDeniedHandler::Forbidden => Response::json_forbidden_msg("Access denied"),
            AccessDeniedHandler::ForbiddenHtml => Response::new(
                403,
                Sanitizer::trust("<h1>403 Forbidden</h1><p>Access Denied.</p>"),
            ),
            AccessDeniedHandler::Redirect(location) => Response::redirect(303, location),
            AccessDeniedHandler::Custom(handler) => handler(ctx),
        }
    }
}
