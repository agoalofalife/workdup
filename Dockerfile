# syntax=docker/dockerfile:1.7

# ---- build stage ---------------------------------------------------------
# rustls + ring (no OpenSSL) and rusqlite `bundled` (static SQLite) mean the
# builder only needs a C toolchain, not libssl/libsqlite dev packages.
FROM rust:1-slim-bookworm AS builder
WORKDIR /app

# protobuf-compiler: prost-wkt-types/temporal protos need `protoc`.
# cmake: aws-lc-rs builds its native crypto with cmake.
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential pkg-config protobuf-compiler libprotobuf-dev cmake \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Cache the registry and target dir so incremental rebuilds are fast. The
# release binary is copied out of the cached target dir inside the same RUN.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/workdup /usr/local/bin/workdup

# ---- runtime stage -------------------------------------------------------
FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
        ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /usr/local/bin/workdup /usr/local/bin/workdup
RUN mkdir -p /app/data

# 8000 = application /metrics + REST API, 9000 = Temporal SDK gRPC metrics
EXPOSE 8000 9000

ENTRYPOINT ["workdup"]
CMD ["--config", "/app/workdup.toml", "run"]
