# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e

FROM rust:bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6 AS base

FROM base AS ripgrep
ARG TARGETARCH
RUN set -eu; \
    case "$TARGETARCH" in \
      arm64) target=aarch64-unknown-linux-gnu; sha=2b661c6ef508e902f388e9098d9c4c5aca72c87b55922d94abdba830b4dc885e ;; \
      amd64) target=x86_64-unknown-linux-musl; sha=1c9297be4a084eea7ecaedf93eb03d058d6faae29bbc57ecdaf5063921491599 ;; \
    esac; \
    asset="ripgrep-15.1.0-$target"; \
    curl -fsSL "https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/$asset.tar.gz" -o /tmp/ripgrep.tar.gz; \
    echo "$sha  /tmp/ripgrep.tar.gz" | sha256sum -c -; \
    tar -xzf /tmp/ripgrep.tar.gz -C / --strip-components=1 "$asset/rg"; \
    rm /tmp/ripgrep.tar.gz; \
    /rg --version

FROM base AS builder
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
COPY --from=ripgrep /rg /bin/rg
COPY conf/talos-prod.yaml /conf/talos.yaml
COPY conf/migrations/ /conf/migrations/
