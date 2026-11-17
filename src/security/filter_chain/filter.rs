use crate::http::response::Response;
use crate::middleware::{Middleware, MiddlewareResult};
use crate::routing::engine::RequestContext;
use sea_orm_migration::async_trait::async_trait;

/// What a security filter decided after inspecting the request.
///
/// `Unauthenticated` and `AccessDenied` exist so a filter can reject without
/// having to know the chain's exception handling: the chain turns them into its
/// [`AuthenticationEntryPoint`](super::AuthenticationEntryPoint) or
/// [`AccessDeniedHandler`](super::AccessDeniedHandler) response.
pub enum FilterResult {
    /// Hand the request to the next filter in the chain.
    Continue,
    /// Stop the chain with this response.
    Respond(Response),
    /// No usable identity — the chain's authentication entry point responds.
    Unauthenticated,
    /// Identity present but not permitted — the chain's access-denied handler
    /// responds.
    AccessDenied,
}

/// One step of a [`SecurityFilterChain`](super::SecurityFilterChain).
///
/// Filters run in registration order during the request phase. The response
/// phase unwinds in reverse order, for exactly the filters that executed, and
/// runs before global middleware `on_response` because the security chain is
/// the innermost stage of the request pipeline.
///
/// Any existing [`Middleware`] is a security filter for free through a blanket
/// implementation — `RateLimitMiddleware`, `IPBlacklistMiddleware`,
/// `CorsMiddleware`, and custom middlewares all drop into a chain unchanged.
#[async_trait]
pub trait SecurityFilter: Send + Sync {
    /// A short label used in diagnostics. Defaults to a generic name.
    fn name(&self) -> &'static str {
        "security-filter"
    }

    /// Inspect or enrich the request before authorization runs.
    async fn on_request(&self, ctx: &mut RequestContext) -> FilterResult;

    /// Observe or mutate the response before it leaves the server.
    async fn on_response(&self, _ctx: &RequestContext, _res: &mut Response) {}
}

/// Bridges the existing middleware surface into chains, including its
/// `MiddlewareState` (session/JWT claims) so a middleware that authenticates
/// keeps working as a chain filter.
#[async_trait]
impl<T: Middleware + ?Sized> SecurityFilter for T {
    async fn on_request(&self, ctx: &mut RequestContext) -> FilterResult {
        match Middleware::on_request(self, ctx).await {
            MiddlewareResult::Next(state) => {
                if let Some(state) = state {
                    if state.session.is_some() {
                        ctx.session = state.session;
                    }
                    if state.claims.is_some() {
                        ctx.claims = state.claims;
                    }
                }
                FilterResult::Continue
            }
            MiddlewareResult::Error(response) => FilterResult::Respond(response),
        }
    }

    async fn on_response(&self, ctx: &RequestContext, res: &mut Response) {
        Middleware::on_response(self, ctx, res).await
    }
}

#[async_trait]
impl SecurityFilter for Box<dyn SecurityFilter> {
    fn name(&self) -> &'static str {
        self.as_ref().name()
    }

    async fn on_request(&self, ctx: &mut RequestContext) -> FilterResult {
        self.as_ref().on_request(ctx).await
    }

    async fn on_response(&self, ctx: &RequestContext, res: &mut Response) {
        self.as_ref().on_response(ctx, res).await
    }
}

#[async_trait]
impl SecurityFilter for std::sync::Arc<dyn SecurityFilter> {
    fn name(&self) -> &'static str {
        self.as_ref().name()
    }

    async fn on_request(&self, ctx: &mut RequestContext) -> FilterResult {
        self.as_ref().on_request(ctx).await
    }

    async fn on_response(&self, ctx: &RequestContext, res: &mut Response) {
        self.as_ref().on_response(ctx, res).await
    }
}
