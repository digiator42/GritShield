//! CSRF and session authentication.
//!
//! CSRF is only meaningful when the browser sends credentials on its own. That
//! is exactly what a signed session cookie does, so the framework pairs the
//! two: session mode enables the anti-forgery guard, and every state-changing
//! request must echo back a token that is stored **server-side** in the
//! session.
//!
//! The token travels in one of two places, and `AuthMiddleware` accepts either:
//!
//! - the `X-CSRF-Token` header (what `fetch` and most clients use)
//! - a `csrf_token` form field (what a plain HTML form posts)
//!
//! Why server-side storage is the point: a token that is merely *present* proves
//! nothing, because an attacker's page can send any value it likes. The token
//! has to match something the attacker cannot read — the session they do not
//! have.
//!
//! Open `src/csrf.rs` next to this file: it is the guide.

use gritshield::http::response::HttpStatus;
use gritshield::http::{Cookie, SameSite};
use gritshield::prelude::*;
use serde_json::json;

/// Any user id is accepted. A real application would look the credential up in
/// a user table here and reject anything that does not match.
fn authenticate(username: &str, password: &str) -> Option<&'static str> {
    match (username, password) {
        ("alice", "correct horse") => Some("user_1001"),
        ("bob", "hunter2") => Some("user_1002"),
        _ => None,
    }
}

pub struct CsrfController;

#[controller("/auth")]
impl CsrfController {
    /// Renders the login form.
    ///
    /// Two things happen here that are worth understanding:
    ///
    /// 1. In session mode an unauthenticated visitor is **given a session** and a
    ///    `GSESSION_ID` cookie before the handler runs. That is what makes the
    ///    CSRF handshake possible.
    /// 2. `get_csrf_token` mints a token into that session on first call and
    ///    returns the same value on every later call, so the form and the
    ///    session can never drift apart.
    ///
    /// ```bash
    /// curl -c jar.txt http://localhost:8080/auth/login
    /// ```
    #[get("/login")]
    pub async fn login_form(ctx: RequestContext) -> Response {
        let token = ctx.get_csrf_token();

        Response::ok(format!(
            r#"<!DOCTYPE html>
<html lang="en">
<head><meta charset="utf-8"><title>Login</title></head>
<body>
  <h1>Sign in</h1>
  <form method="post" action="/auth/session">
    <input type="hidden" name="csrf_token" value="{token}">
    <label>Username <input type="text" name="username" required></label>
    <label>Password <input type="password" name="password" required></label>
    <button type="submit">Sign in</button>
  </form>
</body>
</html>"#
        ))
    }

    /// Hands out the current session's token as JSON.
    ///
    /// The HTML form above embeds the token in the markup, which is all a
    /// browser needs. This endpoint exists so the rest of the guide is scriptable
    /// with `curl` and a cookie jar:
    ///
    /// ```bash
    /// TOKEN=$(curl -s -c jar.txt http://localhost:8080/auth/csrf-token | jq -r .csrf_token)
    /// curl -b jar.txt -X POST http://localhost:8080/auth/session \
    ///   -d "username=alice&password=correct+horse&csrf_token=$TOKEN"
    /// ```
    ///
    /// Note the `-c jar.txt` on the first call: the token is stored in the
    /// session, so the cookie has to come back with every later request or the
    /// framework will see a different, token-less session.
    #[get("/csrf-token")]
    pub async fn csrf_token(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({ "csrf_token": ctx.get_csrf_token() }),
        )
    }

    /// Exchanges credentials for an authenticated session.
    ///
    /// This route is in `public_paths`, so the CSRF guard is skipped — an
    /// anonymous caller with no token reaches the handler and is handed a
    /// session. That is deliberate: you cannot protect a login form with a token
    /// the user does not have yet. The protection that matters starts on the
    /// *next* request, once the browser holds a session.
    ///
    /// ```bash
    /// curl -c jar.txt -X POST http://localhost:8080/auth/session \
    ///   -d 'username=alice&password=correct+horse'
    /// ```
    #[post("/session")]
    pub async fn create_session(ctx: RequestContext) -> Response {
let username = ctx.form.get_plain_str("username").unwrap_or("");
let password = ctx.form.get_plain_str("password").unwrap_or("");

        let user_id = match authenticate(username, password) {
            Some(id) => id,
            None => {
                return Response::json(
                    HttpStatus::Unauthorized,
                    &json!({ "error": "invalid credentials" }),
                )
            }
        };

        // Marks the session as authenticated. The middleware sees `user_id` on
        // the next request and stops redirecting to the login page.
        ctx.login_user_id(user_id);

        Response::json(
            HttpStatus::Ok,
            &json!({
                "logged_in_as": user_id,
                "authenticated": ctx.is_user_authenticated(),
            }),
        )
    }

    /// A protected page. `/auth/me` is **not** in `public_paths`, so an
    /// anonymous request never reaches this handler — the middleware answers
    /// with a redirect first.
    #[get("/me")]
    pub async fn whoami(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({
                "user_id": ctx.get_user_id(),
                "authenticated": ctx.is_user_authenticated(),
            }),
        )
    }

    /// A state-changing request on a protected route.
    ///
    /// `/auth/preferences` is **not** public, so this is where CSRF applies. It
    /// is reached three ways, and two of them are bug reports:
    ///
    /// - no session at all → `303` to `/auth/login`
    /// - session, no token or a wrong one → `403` "Anti-Forgery Token Validation
    ///   Rejected", before this handler runs
    /// - session, matching token → `200`
    ///
    /// ```bash
    /// curl -c jar.txt -X POST http://localhost:8080/auth/session \
    ///   -d 'username=alice&password=correct+horse'
    /// TOKEN=$(curl -s -b jar.txt -c jar.txt http://localhost:8080/auth/csrf-token | jq -r .csrf_token)
    /// curl -b jar.txt -X POST http://localhost:8080/auth/preferences \
    ///   -H "X-CSRF-Token: $TOKEN" -d 'theme=dark'
    /// ```
    #[post("/preferences")]
    pub async fn set_preference(ctx: RequestContext) -> Response {
        let theme = ctx.form.get_plain_str("theme").unwrap_or("light");

        // Signed cookies are HMAC-protected, so a client cannot forge a
        // preference to smuggle something else into the session.
        //
        // `Cookie::new` defaults to `secure: true`, which a browser will refuse
        // to store over plain HTTP. Production code should leave that default
        // alone and terminate TLS in front of the app; the framework's own
        // session cookie makes the same trade-off, flipping `secure` off only
        // when APP_ENV is not "production".
        let cookie = Cookie::new("theme", theme)
            .set_secure(get_env("APP_ENV", "development") == "production")
            .set_same_site(SameSite::Lax);
        ctx.set_signed_cookie(cookie);

        Response::json(
            HttpStatus::Ok,
            &json!({
                "saved": theme,
                // Always null here. Reading a cookie inspects the *request*,
                // and this response has not been sent yet, so the value the
                // browser holds is still the old one. Come back on the next
                // request to see it.
                "read_back": ctx.get_signed_cookie("theme"),
            }),
        )
    }

    /// Reading both accessors on the following request shows what signing buys:
    ///
    /// ```json
    /// {"signed":"dark",
    ///  "unsigned":"dark.a58979a1cbba7c05cd24d3d2ca2191a82cb19e4942a622d195d6ffa61c8962f5"}
    /// ```
    ///
    /// `get_signed_cookie` verifies the HMAC and hands back `"dark"`.
    /// `get_cookie` returns the wire value untouched — the payload plus its
    /// signature — which is why anything security-relevant should be read
    /// through the signed accessor.
    #[get("/preferences")]
    pub async fn read_preference(ctx: RequestContext) -> Response {
        Response::json(
            HttpStatus::Ok,
            &json!({
                "signed": ctx.get_signed_cookie("theme"),
                "unsigned": ctx.get_cookie("theme"),
            }),
        )
    }

    /// Drops the session cookie. The store entry becomes stale, and the
    /// middleware notices and clears it on the following request — which is why
    /// logging out needs nothing more than this.
    #[post("/logout")]
    pub async fn logout(ctx: RequestContext) -> Response {
        ctx.remove_cookie("GSESSION_ID");

        Response::json(HttpStatus::Ok, &json!({ "logged_out": true }))
    }
}