# Stage 1: Build the Rust application using the latest stable version
FROM rust:alpine AS builder
# Install dependencies needed for compiling and fetching git crates
RUN apk add --no-cache musl-dev git
WORKDIR /app

# Copy dependency configurations first
COPY Cargo.toml Cargo.lock ./

# Copy the server source files
COPY src ./src

# Build the release binary
RUN cargo build --release

# Stage 2: Minimal runtime environment
FROM alpine:latest
WORKDIR /app
RUN apk add --no-cache ca-certificates

# Copy the compiled binary from the builder stage
COPY --from=builder /app/target/release/docs .

# Copy only the necessary web folders for runtime
COPY docs_content ./docs_content
COPY static ./static

EXPOSE 10000

# Start the documentation server
CMD ["./docs"]
