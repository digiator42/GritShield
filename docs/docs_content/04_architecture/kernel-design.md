
## Core Components

### Request Pipeline
TCP Stream → Request Parser → Cookie Jar → Middleware Stack → Router → Handler → Response

### Memory Safety

GritShield leverages Rust's ownership system:

- No garbage collection pauses
- Thread-safe without data races
- Memory leaks are compile-time errors
- Buffer overflows impossible

### Async Runtime

Built on `tokio` with a multi-threaded work-stealing scheduler:

```rust
tokio::spawn(async move {
    handle_connection(stream, peer_addr, router).await
});
```

### Security Kernel

**Trust Boundary**

The security kernel is the only component that can:

- Create UntrustedString from raw input
- Convert UntrustedString to SafeHtml via sanitization
- Sign and verify cookies with HMAC
- Validate JWT tokens

**Isolation Layers**

- Protocol Layer - Raw HTTP parsing with timeouts
- Security Layer - All validation and sanitization
- Application Layer - Your business logic
- Presentation Layer - Safe HTML output only

### Performance Design

- Zero-copy parsing where possible
- Arc sharing for database connections
- Lock-free telemetry counters
- Memory pool for session storage
- Trie-based O(n) routing where n = path segments

### Error Isolation

Panics are caught at the request level:

```rust
match AssertUnwindSafe(response_future).catch_unwind().await {
    Ok(response) => response,
    Err(panic) => handle_panic(panic),
}
```

One crashing request never brings down the server.