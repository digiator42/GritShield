//! Spring-Security-style filter chains for GritShield.
//!
//! A [`SecurityFilterChain`] groups filters and authorization rules behind a
//! request matcher. The router walks chains in registration order; the first
//! chain whose matcher accepts the request governs it:
//!
//! ```text
//! request ──▶ chain #1 matcher? ──yes──▶ filters ──▶ authorization ──▶ handler
//!                │ no
//!                ▼
//!            chain #2 matcher? ──yes──▶ ...
//!                │ no
//!                ▼
//!            untouched: middleware + handler as before
//! ```
//!
//! This is the composable counterpart to [`AuthMiddleware`](crate::middleware::AuthMiddleware):
//! instead of choosing "session mode" or "JWT mode" for the whole application,
//! each chain states its own matcher, filters, rules, and exception handling.
//!
//! ```rust,no_run,ignore
//! use gritshield::prelude::*;
//!
//! let api = SecurityFilterChain::builder()
//!     .name("api")
//!     .matching("/api/**")
//!     .jwt_auth(JwtHandler::new(&secret))
//!     .authorize_public("/api/health")
//!     .authorize_role("Admin", "/api/admin/**")
//!     .any_request(AccessRule::authenticated())
//!     .entry_point(AuthenticationEntryPoint::bearer())
//!     .build();
//!
//! let web = SecurityFilterChain::builder()
//!     .name("web")
//!     .matching("/**")
//!     .session_auth()
//!     .csrf()
//!     .authorize_public("/login")
//!     .any_request(AccessRule::authenticated())
//!     .entry_point(AuthenticationEntryPoint::redirect("/login"))
//!     .build();
//!
//! let router = Router::new()
//!     .add_security_chain(api)
//!     .add_security_chain(web);
//! ```

mod access;
mod auth;
mod chain;
mod entry;
mod filter;

pub use access::{is_authenticated, AccessDecision, AccessRule, AuthorizationRule};
pub use auth::{AuthMode, AuthenticationFilter, CsrfFilter};
pub(crate) use chain::ChainOutcome;
pub use chain::{
    SecurityChainRegistration, SecurityChainResult, SecurityFilterChain,
    SecurityFilterChainBuilder,
};
pub use entry::{AccessDeniedHandler, AuthenticationEntryPoint};
pub use filter::{FilterResult, SecurityFilter};
