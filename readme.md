





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