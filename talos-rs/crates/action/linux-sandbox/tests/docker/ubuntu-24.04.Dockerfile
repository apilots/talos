FROM ubuntu:24.04@sha256:561618e2c15bf2397621dd04f96926663a3b5616c189cf7e38db7e82f5c538ea AS runtime

ARG JUST_VERSION=1.53.0
ARG NEXTEST_VERSION=0.9.149
ARG JUST_SHA256=7fedeb22c7e14d9ef1551e8b793700866d80f409f9884b0e80ebb65c11d4874d
ARG NEXTEST_SHA256=32d1d8c0567b7a80bee2bad8bb09125c846da96bc009b2de11ec08557290c770
LABEL org.opencontainers.image.source="https://github.com/apilots/talos"
LABEL org.opencontainers.image.description="Pinned Talos Linux sandbox test environment"

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        bubblewrap ca-certificates curl libssl3t64 python3 ripgrep \
    && rm -rf /var/lib/apt/lists/*

RUN curl -fsSL "https://github.com/casey/just/releases/download/${JUST_VERSION}/just-${JUST_VERSION}-x86_64-unknown-linux-musl.tar.gz" -o /tmp/just.tar.gz \
    && echo "${JUST_SHA256}  /tmp/just.tar.gz" | sha256sum -c - \
    && tar -xzf /tmp/just.tar.gz -C /usr/local/bin just \
    && curl -fsSL "https://github.com/nextest-rs/nextest/releases/download/cargo-nextest-${NEXTEST_VERSION}/cargo-nextest-${NEXTEST_VERSION}-x86_64-unknown-linux-gnu.tar.gz" -o /tmp/nextest.tar.gz \
    && echo "${NEXTEST_SHA256}  /tmp/nextest.tar.gz" | sha256sum -c - \
    && tar -xzf /tmp/nextest.tar.gz -C /usr/local/bin cargo-nextest \
    && rm /tmp/just.tar.gz /tmp/nextest.tar.gz

ENV PATH="/root/.cargo/bin:${PATH}"
WORKDIR /workspace/talos/talos-rs
CMD ["just", "test", "-p", "talos-linux-sandbox"]

# Standalone acceptance without supplied archives retains its original compile path.
FROM runtime AS standalone
ARG RUST_VERSION=1.99.0
RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends build-essential libssl-dev pkg-config protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain ${RUST_VERSION}
