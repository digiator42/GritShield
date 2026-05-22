# Stage 1: Build the Rust application
FROM rust:alpine AS builder
RUN apk add --no-cache musl-dev git
WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY docs_content ./docs_content
COPY src ./src

RUN cargo build --release

# Stage 2: Minimal runtime environment
FROM alpine:latest
WORKDIR /app
RUN apk add --no-cache ca-certificates

# 1. Copy the compiled binary
COPY --from=builder /app/target/release/docs .

# 2. Copy the folders needed at runtime
COPY docs_content ./docs_content
COPY static ./static

# 3. Copy src/pages
COPY src/pages ./src/pages

EXPOSE 10000

CMD ["./docs"]
