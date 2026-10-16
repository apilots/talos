# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e

ARG RUST_IMAGE=rust:bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6
ARG RUNTIME_IMAGE=ubuntu:24.04
ARG PRODUCT_STAGE=source

FROM ${RUST_IMAGE} AS rust-builder
WORKDIR /workspace
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libsqlite3-dev libssl-dev pkg-config protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
COPY talos/talos-rs ./talos/talos-rs
ARG TARGETPLATFORM
RUN --mount=type=cache,id=talos-worker-target-${TARGETPLATFORM},target=/workspace/talos/talos-rs/target \
    --mount=type=cache,id=talos-worker-cargo-registry,target=/usr/local/cargo/registry \
    cargo build --locked --manifest-path talos/talos-rs/Cargo.toml \
    -p talos-worker-bin -p talos-linux-sandbox --release \
    && mkdir -p /artifacts \
    && cp talos/talos-rs/target/release/talos-worker talos/talos-rs/target/release/talos-linux-sandbox-launcher /artifacts/

FROM rust-builder AS source

FROM scratch AS packaged
COPY talos/.release-product/bin/talos-worker talos/.release-product/bin/talos-linux-sandbox-launcher /artifacts/

FROM ${PRODUCT_STAGE} AS product

FROM ${RUNTIME_IMAGE} AS runtime
ARG VERSION=dev
ARG REVISION=unknown
ARG DEBIAN_FRONTEND=noninteractive
LABEL org.opencontainers.image.title="Talos Worker" \
      org.opencontainers.image.description="User-owned Talos execution Worker" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.source="https://github.com/apilots/talos" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"
RUN TZ=Etc/UTC apt-get update && TZ=Etc/UTC apt-get install -y --no-install-recommends \
    bash bubblewrap ca-certificates curl git libsqlite3-0 libssl3t64 ripgrep tini tzdata \
    && rm -rf /var/lib/apt/lists/*
RUN groupadd --gid 10001 talos \
    && useradd --uid 10001 --gid talos --home-dir /home/talos --create-home --shell /bin/bash talos \
    && chmod 0755 /home/talos \
    && install -d -o talos -g talos /opt/talos/bin /opt/talos/conf /home/talos/.talos
COPY --from=product /artifacts/talos-worker /opt/talos/bin/talos-worker
COPY --from=product /artifacts/talos-linux-sandbox-launcher /opt/talos/bin/talos-linux-sandbox-launcher
COPY talos/conf/talos-prod.yaml /opt/talos/conf/talos.yaml
COPY talos/LICENSE talos/NOTICE talos/THIRD_PARTY_NOTICES /usr/share/licenses/talos/
RUN /opt/talos/bin/talos-worker --version \
    && test -s /usr/share/licenses/talos/LICENSE \
    && test -s /usr/share/licenses/talos/NOTICE \
    && test -s /usr/share/licenses/talos/THIRD_PARTY_NOTICES \
    && /opt/talos/bin/talos-linux-sandbox-launcher --probe-seccomp
ENV HOME=/home/talos \
    SHELL=/bin/bash \
    TALOS_RESOURCE_DIR=/opt/talos \
    PATH=/opt/talos/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
USER talos
WORKDIR /home/talos
VOLUME ["/home/talos/.talos"]
# Outbound registration requires no published port. Direct RPC is opt-in config.
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/opt/talos/bin/talos-worker"]
