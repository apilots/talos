# syntax=docker/dockerfile:1.7@sha256:a57df69d0ea827fb7266491f2813635de6f17269be881f696fbfdf2d83dda33e

ARG NODE_IMAGE=node:24-bookworm
ARG RUST_IMAGE=rust:bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6
ARG RUNTIME_IMAGE=ubuntu:24.04

FROM ${NODE_IMAGE} AS web-builder
WORKDIR /workspace/talos

RUN corepack enable && corepack prepare pnpm@9.14.4 --activate

COPY talos/talos-app ./talos-app
RUN pnpm --dir talos-app install --frozen-lockfile
RUN pnpm --dir talos-app --filter talos-web build

FROM ${RUST_IMAGE} AS rust-builder
ARG TARGETPLATFORM
WORKDIR /workspace

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    libsqlite3-dev \
    libssl-dev \
    pkg-config \
    protobuf-compiler \
  && rm -rf /var/lib/apt/lists/*

COPY talos/talos-rs ./talos/talos-rs

RUN --mount=type=cache,id=talos-worker-target-${TARGETPLATFORM},target=/workspace/talos/talos-rs/target \
    --mount=type=cache,id=talos-worker-cargo-registry,target=/usr/local/cargo/registry \
    cargo build --locked --manifest-path talos/talos-rs/Cargo.toml \
      -p talos-server -p talos-worker-bin -p talos-linux-sandbox --release \
    && mkdir -p /artifacts \
    && cp talos/talos-rs/target/release/talos-server talos/talos-rs/target/release/talos-worker \
      talos/talos-rs/target/release/talos-linux-sandbox-launcher /artifacts/

FROM ${RUNTIME_IMAGE} AS runtime

ARG VERSION=dev
ARG REVISION=unknown
ARG DEBIAN_FRONTEND=noninteractive

LABEL org.opencontainers.image.title="Talos Server" \
      org.opencontainers.image.description="Talos HTTP API and WebUI server" \
      org.opencontainers.image.source="https://github.com/apilots/talos" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"

RUN TZ=Etc/UTC apt-get update && TZ=Etc/UTC apt-get install -y --no-install-recommends \
    bash \
    bubblewrap \
    ca-certificates \
    curl \
    git \
    libsqlite3-0 \
    libssl3t64 \
    ripgrep \
    tini \
    tzdata \
  && rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 10001 talos \
  && useradd --uid 10001 --gid talos --home-dir /home/talos --create-home --shell /bin/bash talos \
  && chmod 0755 /home/talos \
  && install -d -o talos -g talos /opt/talos/bin /opt/talos/conf /opt/talos/webui /home/talos/.talos

COPY --from=rust-builder /artifacts/talos-server /opt/talos/bin/talos-server
COPY --from=rust-builder /artifacts/talos-worker /opt/talos/bin/talos-worker
COPY --from=rust-builder /artifacts/talos-linux-sandbox-launcher /opt/talos/bin/talos-linux-sandbox-launcher
COPY --from=web-builder /workspace/talos/talos-app/web/dist/ /opt/talos/webui/
COPY talos/conf/talos-cloud.yaml /opt/talos/conf/talos.yaml
COPY talos/conf/migrations /opt/talos/conf/migrations

RUN chmod 755 /opt/talos/bin/talos-server /opt/talos/bin/talos-worker /opt/talos/bin/talos-linux-sandbox-launcher \
  && chown -R talos:talos /home/talos \
  && /opt/talos/bin/talos-linux-sandbox-launcher --probe-seccomp \
  && bwrap --version \
  && /opt/talos/bin/talos-server --version \
  && /opt/talos/bin/talos-worker --version

ENV HOME=/home/talos \
    SHELL=/bin/bash \
    TALOS_RESOURCE_DIR=/opt/talos \
    PATH=/opt/talos/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin

USER talos
WORKDIR /home/talos

EXPOSE 8080
VOLUME ["/home/talos/.talos"]

HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8080/health >/dev/null || exit 1

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/opt/talos/bin/talos-server"]
