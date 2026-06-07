# Security Guide

GritShield eliminates OWASP Top 10 vulnerabilities by design. This section covers each security feature in depth.

## OWASP Top 10 Coverage

| # | Vulnerability | GritShield Protection |
|---|---------------|----------------------|
| 1 | Broken Access Control | AuthMiddleware + session/JWT |
| 2 | Cryptographic Failures | HMAC-SHA256, environment secrets |
| 3 | Injection (SQL) | SeaORM prepared statements |
| 4 | Insecure Design | Security-first architecture |
| 5 | Security Misconfiguration | Production defaults |
| 6 | Vulnerable Components | Minimal dependencies, audited |
| 7 | Identification Failures | Signed sessions, JWT validation |
| 8 | Software Integrity | Rust's safety guarantees |
| 9 | Monitoring Failures | Telemetry + logging middleware |
| 10 | SSRF | No automatic external requests |

## Security Headers

GritShield includes these security headers by default:
- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`

## Best Practices

1. Always use `Sanitizer::encode()` for user input
2. Enable CSRF for all forms
3. Use HTTPS in production
4. Rotate JWT secrets regularly
5. Monitor rate limit violations
6. Keep dependencies updated

## AuthMiddleware – Session vs JWT

By using AuthMiddleware you have a full authentication system that exposes only `/login` & `/register` routes, sets signed `hmac cookies`, generates a new `CSRF` token immediately, logs out user automatically and redirects unauthenticated users to `/login`.

**Session mode (default)**

```rust
let auth = AuthMiddleware::new_session(
    vec!["/login".to_string(), "/register".to_string()],
    Some("/login")
);

router = router.add_middleware(auth);
```

**Features:**

- Creates a signed `GSESSION_ID` cookie.
- Stores user data in an in‑memory `SessionStore`.
- Use `ctx.login_user_id("123")` to authenticate.
- `ctx.is_user_authenticated()` checks login state.


## CSRF Protection

Enabled automatically in session mode.

HTML forms must include csrf_token value:

```rust
let token = ctx.get_csrf_token();

// using maud
render!(ctx, "title", html! {
    input type="hidden" id="global-csrf-token" value=(csrf_token);
})
```

## Cookies – Signed & Secure

```rust
// Read a signed cookie
if let Some(val) = ctx.get_signed_cookie("user_pref") {
    // ...
}

// Set a signed cookie
let cookie = Cookie::new("pref", "dark_mode")
    .set_secure(cfg!(production))
    .set_same_site(SameSite::Lax);

ctx.set_signed_cookie(cookie);

// Delete a cookie
ctx.remove_cookie("pref");
```

Unsigned cookies are also available:

```rust
ctx.get_cookie()
ctx.set_cookie()
```

## XSS Prevention

All user input arrives as `UntrustedString`.

To display safely:

```rust
let safe_html = Sanitizer::encode(untrusted_string);
```

To return trusted HTML:

```rust
Sanitizer::trust("...")
```

Only use for strings you fully control.

## Rate Limiting

```rust
let limiter = RateLimiter::new(50, Duration::from_secs(60));

let rate_middleware = RateLimitMiddleware { limiter };

router = router.add_middleware(rate_middleware);
```

## IP Blacklisting

```rust
let blacklist = IPBlacklistMiddleware::new(vec![
    "192.168.1.100",
    "10.0.0.5"
]);

router = router.add_middleware(blacklist);
```