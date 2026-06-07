# Deployment Guide

This section covers deploying GritShield applications to production environments.

## Before You Deploy

✓ Set `APP_ENV=production`  
✓ Generate strong `JWT_SECRET` (32+ random bytes)  
✓ Configure production database  
✓ Enable HTTPS/TLS  
✓ Set up monitoring  
✓ Configure logging  

## Quick Checklist

- [ ] Code compiles with `--release`
- [ ] Environment variables set
- [ ] Database migrations run
- [ ] Static assets collected
- [ ] Health check endpoint working
- [ ] Rate limits configured
- [ ] IP blacklist populated
- [ ] Logging configured for production

## Recommended Stack
[Client] → [CloudFlare/CDN] → [nginx] → [GritShield] → [PostgreSQL]
↓
[Redis (sessions)]

## Health Check Endpoint

Implement a health check:

```rust
#[get("/health")]
async fn health(ctx: RequestContext) -> Response {
    Response::new(200, Sanitizer::trust("OK"))
}
```

## Monitoring Metrics

Track using telemetry:

```rust
// Export metrics via Prometheus endpoint
#[get("/metrics")]
async fn metrics(ctx: RequestContext) -> Response {
    let metrics = format!(
        "active_connections {}\ntotal_blocked_ips {}\nrate_limited_reqs {}",
        ctx.telemetry.active_connections.load(Ordering::SeqCst),
        ctx.telemetry.total_blocked_ips.load(Ordering::SeqCst),
        ctx.telemetry.total_rate_limited_reqs.load(Ordering::SeqCst),
    );
    Response::new(200, Sanitizer::trust(&metrics))
}
```