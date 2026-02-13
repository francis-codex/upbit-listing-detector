# Build stage
FROM rust:1.85-slim AS builder

WORKDIR /app

# Cache dependencies by copying manifests first
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release 2>/dev/null || true

# Copy actual source and build
COPY . .
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/upbit-listing-detector /usr/local/bin/upbit-listing-detector
COPY config.toml /etc/upbit-detector/config.toml

ENV RUST_LOG=info

CMD ["upbit-listing-detector"]
