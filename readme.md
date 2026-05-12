





```bash
resilience_core/           # Your Project Root
├── Cargo.toml             # Project manifest
├── src/
│   ├── main.rs            # Entry point (Server startup)
│   ├── lib.rs             # Library root (Exposes framework modules)
│   ├── core/              # The "Kernel" (Sockets & I/O)
│   │   ├── mod.rs
│   │   ├── server.rs      # TcpListener & ThreadPool
│   │   └── connection.rs  # Connection timeouts & Keep-alive
│   ├── protocol/          # HTTP Logic
│   │   ├── mod.rs
│   │   ├── request.rs     # SecureRequest struct & Parsing
│   │   └── response.rs    # SecureResponse & Header logic
│   ├── security/          # The "Firewall"
│   │   ├── mod.rs
│   │   ├── middleware.rs  # Trait definitions for guards
│   │   ├── xss.rs         # Newtype pattern (SafeHtml)
│   │   └── jwt.rs         # Token verification logic
│   └── routing/           # Route matching
│       ├── mod.rs
│       └── trie.rs        # Trie-based router for performance
└── tests/                 # Integration tests
    ├── integration_test.rs
    └── security_tests.rs  # Exploit simulations
``` 

cmds:
```bash
rustup toolchain install nightly
```


### Test Results Summary
The framework was subjected to a suite of security-focused unit and integration tests. The findings are as follows:

*   **XSS Parsing:** The `SafeHtml` newtype pattern and sanitization logic correctly identified and neutralized malicious script injections in request payloads.
*   **Header Timeout:** The server successfully dropped connections that failed to send complete HTTP headers within the configured grace period, mitigating initial-stage Slowloris attempts.
*   **Body Timeout (Slowloris):** The implementation correctly handles slow data trickling.
*   **Security Observation:** During testing of `read_exact` on the request body, it was noted that while timeouts are enforced, a malicious actor could potentially occupy a worker thread for the duration of the timeout period. Future iterations may explore async I/O or non-blocking reads to prevent thread exhaustion during high-latency body transmissions.
