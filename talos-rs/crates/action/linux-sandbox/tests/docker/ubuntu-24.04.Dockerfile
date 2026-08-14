FROM ubuntu:24.04@sha256:561618e2c15bf2397621dd04f96926663a3b5616c189cf7e38db7e82f5c538ea

ARG RUST_VERSION=1.88.0

RUN apt-get update \
    && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
        bubblewrap \
        build-essential \
        ca-certificates \
        curl \
        libssl-dev \
        pkg-config \
        python3 \
        ripgrep \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain ${RUST_VERSION} \
    && /root/.cargo/bin/rustup component add clippy \
    && /root/.cargo/bin/cargo install just --version 1.53.0 --locked \
    && /root/.cargo/bin/cargo install cargo-nextest --version 0.9.100 --locked

ENV PATH="/root/.cargo/bin:${PATH}"
WORKDIR /workspace/talos/talos-rs

CMD ["just", "test", "-p", "talos-linux-sandbox"]
