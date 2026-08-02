use crate::middleware::{MiddlewareResult, MiddlewareState};
use crate::routing::engine::{RequestContext, Router};
use std::time::Duration;

impl Router {
    pub fn run_middlewares(&self, ctx: &mut RequestContext) -> MiddlewareResult {
        // Initialize an empty accumulator state packer
        let mut accumulated_state = MiddlewareState {
            session: None,
            claims: None,
            session_was_stale: false,
        };

        for middleware in &self.middlewares {
            match middleware.execute(ctx) {
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
                MiddlewareResult::Error(res) => return MiddlewareResult::Error(res),
            }
        }

        // Return the perfectly merged collection of sessions and claims
        MiddlewareResult::Next(Some(accumulated_state))
    }

    pub async fn run_after_hooks(&self, ctx: &RequestContext, status: u16, duration: Duration) {
        for hook in &self.after_hooks {
            hook.call(ctx, status, duration).await;
        }
    }
}
