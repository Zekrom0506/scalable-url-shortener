# ---- Builder Stage ----
FROM rust:1.88-alpine AS builder

# Install build dependencies
RUN apk add --no-cache musl-dev build-base

WORKDIR /app

# Cache dependencies: build once with a dummy main, then with the real source
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src benches && echo "fn main() {}" > src/main.rs && echo "" > src/lib.rs \
    && echo "fn main() {}" > benches/url_shortener_benchmark.rs \
    && cargo build --release && rm -rf src benches

COPY src ./src
COPY benches ./benches
# Native build for whatever CPU Docker runs on (arm64 on Apple Silicon, amd64 elsewhere).
# Alpine targets musl, so the binary is statically linked.
RUN touch src/main.rs src/lib.rs && cargo build --release

# ---- Runtime Stage ----
FROM alpine:3.20

RUN apk add --no-cache ca-certificates wget && adduser -D appuser
WORKDIR /app
COPY --from=builder /app/target/release/url_shortener .
COPY .env .env

USER appuser
EXPOSE 8000
CMD ["./url_shortener"]
