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

## Topics

- [Rate Limiting](/docs/security/rate-limiting) - Prevent DoS attacks
- [RBAC](/docs/security/rbac) - Role-based access control
- [CSRF Protection](/docs/security/csrf-protection) - Cross-site request forgery
- [XSS Prevention](/docs/security/xss-prevention) - Cross-site scripting

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