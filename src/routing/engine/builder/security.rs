use crate::http::response::Response;
use crate::routing::engine::{RequestContext, Router};
use crate::security::filter_chain::{ChainOutcome, SecurityChainResult, SecurityFilterChain};

impl Router {
    /// Append a security chain.
    ///
    /// Chains are evaluated in registration order and the first chain whose
    /// matcher accepts a request handles it. Register the broadest chain last.
    pub fn add_security_chain(mut self, chain: SecurityFilterChain) -> Self {
        self.security_chains.push(chain);
        self
    }

    /// Offer a request to the security chains.
    ///
    /// Returns [`SecurityChainResult::NoChain`] when nothing matched, so a
    /// router without chains behaves exactly as it did before.
    pub(crate) async fn run_security_chains(
        &self,
        ctx: &mut RequestContext,
    ) -> SecurityChainResult {
        for (chain_index, chain) in self.security_chains.iter().enumerate() {
            if !chain.matches(ctx) {
                continue;
            }

            return match chain.run(ctx).await {
                ChainOutcome::Continue { filters } => SecurityChainResult::Continue {
                    chain: chain_index,
                    filters,
                },
                ChainOutcome::Respond { filters, response } => SecurityChainResult::Respond {
                    chain: chain_index,
                    filters,
                    response,
                },
            };
        }

        SecurityChainResult::NoChain
    }

    /// Unwind the response phase of the filters that ran in `chain`.
    ///
    /// The security stage is innermost in the request pipeline, so its response
    /// phase runs before global middleware `on_response` handlers.
    pub(crate) async fn run_security_on_response(
        &self,
        ctx: &RequestContext,
        res: &mut Response,
        chain: usize,
        filters: usize,
    ) {
        if let Some(chain) = self.security_chains.get(chain) {
            chain.run_on_response(ctx, res, filters).await;
        }
    }
}
