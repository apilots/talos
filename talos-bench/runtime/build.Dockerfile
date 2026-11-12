# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e

FROM rust:bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6 AS builder
ARG TARGETPLATFORM
WORKDIR /workspace

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libsqlite3-dev libssl-dev pkg-config protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

COPY talos-rs ./talos/talos-rs
COPY deploy/docker/talos-worker/chromium-seccomp.json ./talos/deploy/docker/talos-worker/chromium-seccomp.json

RUN --mount=type=cache,id=talos-worker-target-${TARGETPLATFORM},target=/workspace/talos/talos-rs/target,sharing=locked \
    --mount=type=cache,id=talos-worker-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=talos-bench-cargo-git,target=/usr/local/cargo/git \
    cargo build --locked --release --manifest-path talos/talos-rs/Cargo.toml \
      -p talos-tui -p talos-linux-sandbox \
    && mkdir -p /artifacts/bin \
    && cp talos/talos-rs/target/release/talos \
      talos/talos-rs/target/release/talos-linux-sandbox-launcher /artifacts/bin/

FROM scratch
COPY --from=builder /artifacts/ /
COPY conf/talos-prod.yaml /conf/talos.yaml
COPY conf/migrations/ /conf/migrations/
