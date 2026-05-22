# Stage 1: Build the Rust application
FROM rust:1.75-alpine AS builder
RUN apk add --no-cache musl-dev git
WORKDIR /app

# Copy dependency configurations first to cache them
COPY Cargo.toml Cargo.lock ./

# Copy the server source files
COPY src ./src

# Build the release binary
RUN cargo build --release

# Stage 2: Minimal runtime environment
FROM alpine:latest
WORKDIR /app

# Install certificates for secure outbound web requests if needed
RUN apk add --no-cache ca-certificates

# Copy the compiled binary from the builder stage
COPY --from=builder /app/target/release/docs .

# Copy only the necessary web folders for runtime
COPY docs_content ./docs_content
COPY static ./static

# Render injects a PORT environment variable. 
# Ensure your Rust app reads the PORT env or defaults to 10000.
EXPOSE 10000

# Start the documentation server
CMD ["./docs"]