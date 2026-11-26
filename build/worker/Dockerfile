# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e

ARG RUST_IMAGE=rust:bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6
ARG RUNTIME_IMAGE=ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac
ARG BROWSER_SUPPLIER_IMAGE=node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5
ARG PRODUCT_STAGE=source

FROM ${BROWSER_SUPPLIER_IMAGE} AS browser-supplier
COPY talos/build/container/prepare-browser.sh /build/container/prepare-browser.sh
RUN bash /build/container/prepare-browser.sh
COPY talos/talos-app/shared/app-icons /talos-app/shared/app-icons
COPY talos/talos-app/shared/browser-home /talos-app/shared/browser-home
RUN node /talos-app/shared/app-icons/generate.mjs /opt/talos/browser/desktop

FROM ${BROWSER_SUPPLIER_IMAGE} AS official-mcps
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates && rm -rf /var/lib/apt/lists/*
COPY talos/build/plugins /build/plugins
RUN node /build/plugins/snapshot.mjs /snapshot

FROM ${RUST_IMAGE} AS rust-builder
WORKDIR /workspace
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libsqlite3-dev libssl-dev pkg-config protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*
COPY talos/talos-rs ./talos/talos-rs
COPY talos/deploy/docker/talos-worker/chromium-seccomp.json ./talos/deploy/docker/talos-worker/chromium-seccomp.json
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
LABEL org.opencontainers.image.title="Talos Worker" \
      org.opencontainers.image.description="User-owned Talos execution Worker" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.source="https://github.com/apilots/talos" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"
COPY --from=browser-supplier /opt/talos/browser /opt/talos/browser
COPY --from=official-mcps /snapshot /opt/talos/plugins/apilots-mcps
COPY talos/build/worker/browser /opt/talos/browser
COPY talos/talos-rs/crates/action/browser/resources/browser_driver.py /opt/talos/browser/browser_driver.py
COPY talos/build/container/install-runtime.sh /build/container/install-runtime.sh
RUN bash /build/container/install-runtime.sh && rm -rf /build/container
COPY --from=product /artifacts/talos-worker /opt/talos/bin/talos-worker
COPY --from=product /artifacts/talos-linux-sandbox-launcher /opt/talos/bin/talos-linux-sandbox-launcher
COPY talos/conf/talos-prod.yaml /opt/talos/conf/talos.yaml
COPY talos/build/worker/runner.yaml /tmp/talos-worker-runner.yaml
# Package a complete shared schema with Docker-specific runner defaults.
# Startup Upgrade uses this YAML; no image marker selects the transport.
RUN python3 - <<'PYTHON'
from pathlib import Path
config = Path("/opt/talos/conf/talos.yaml")
runner = Path("/tmp/talos-worker-runner.yaml")
source = config.read_text()
marker = "  runner: null\n"
if source.count(marker) != 1:
    raise SystemExit("Expected one runner default in the shared template")
config.write_text(source.replace(marker, runner.read_text()))
runner.unlink()
PYTHON
COPY talos/LICENSE talos/NOTICE talos/THIRD_PARTY_NOTICES /usr/share/licenses/talos/
RUN /opt/talos/bin/talos-worker --version \
    && test -s /usr/share/licenses/talos/LICENSE \
    && test -s /usr/share/licenses/talos/NOTICE \
    && test -s /usr/share/licenses/talos/THIRD_PARTY_NOTICES \
    && /opt/talos/bin/talos-linux-sandbox-launcher --probe-seccomp
ENV HOME=/home/talos \
    LANG=en_US.UTF-8 \
    SHELL=/bin/bash \
    TALOS_RESOURCE_DIR=/opt/talos \
    TALOS_WORKER_PACKAGING=official-container \
    PATH=/opt/talos/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
USER talos
RUN rg --version
WORKDIR /home/talos
VOLUME ["/home/talos/.talos"]
# Docker defaults to Worker-initiated gRPC Connect; no inbound port is published.
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/opt/talos/bin/talos-worker"]
