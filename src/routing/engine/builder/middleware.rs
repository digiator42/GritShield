use crate::http::response::Response;
use crate::middleware::{MiddlewareResult, MiddlewareState};
use crate::routing::engine::{RequestContext, Router};
use std::time::Duration;

impl Router {
    /// Run the request phase of every middleware in registration order,
    /// returning how many actually executed.
    ///
    /// The count is what lets the response phase run `on_response` only for
    /// middlewares that saw this request: on a short-circuit at index `i` the
    /// count is `i + 1`, since the rest never executed.
    pub async fn run_middlewares_counted(
        &self,
        ctx: &mut RequestContext,
    ) -> (usize, MiddlewareResult) {
        // Initialize an empty accumulator state packer
        let mut accumulated_state = MiddlewareState {
            session: None,
            claims: None,
            session_was_stale: false,
        };

        for (index, middleware) in self.middlewares.iter().enumerate() {
            match middleware.on_request(ctx).await {
                MiddlewareResult::Next(maybe_state) => {
                    if let Some(state) = maybe_state {
                        // Merge fields dynamically without overwriting existing ones with None
                        if state.session.is_some() {
                            accumulated_state.session = state.session;
                        }
                        if state.claims.is_some() {
                            accumulated_state.claims = state.claims;
                        }
                    }
                    continue;
                }
                MiddlewareResult::Error(res) => return (index + 1, MiddlewareResult::Error(res)),
            }
        }

        // Return the perfectly merged collection of sessions and claims
        (
            self.middlewares.len(),
            MiddlewareResult::Next(Some(accumulated_state)),
        )
    }

    pub async fn run_middlewares(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        self.run_middlewares_counted(ctx).await.1
    }

    /// Run the response phase of the first `ran` middlewares, in reverse
    /// registration order (onion unwinding: the first-registered middleware
    /// finalizes last). See [`Middleware::on_response`](crate::middleware::Middleware).
    pub async fn run_on_response(
        &self,
        ctx: &RequestContext,
        res: &mut Response,
        ran: usize,
    ) {
        for middleware in self.middlewares.iter().take(ran).rev() {
            middleware.on_response(ctx, res).await;
        }
    }

    pub async fn run_after_hooks(&self, ctx: &RequestContext, status: u16, duration: Duration) {
        for hook in &self.after_hooks {
            hook.call(ctx, status, duration).await;
        }
    }
}
