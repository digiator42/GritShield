# Stage 1: Build the Rust application
FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev git
WORKDIR /app

# Copy dependency configurations
COPY Cargo.toml Cargo.lock ./

# Copy the docs_content folder and src folder into the build root
COPY docs_content ./docs_content
COPY src ./src

# Build the release binary (your code can now access docs_content here)
RUN cargo build --release

# Stage 2: Minimal runtime environment
FROM alpine:latest
WORKDIR /app
RUN apk add --no-cache ca-certificates

# Copy the compiled binary from the builder stage
COPY --from=builder /app/target/release/docs .

# Copy web assets needed at runtime next to the binary
COPY docs_content ./docs_content
COPY static ./static

EXPOSE 10000

# Start the documentation server
CMD ["./docs"]
