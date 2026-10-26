# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e

ARG NODE_IMAGE=node:24.21.0-bookworm@sha256:3d27e5c11e5786e309ec3e03f93ae536eb36e6e5eb3714d5eb3300a36157add0
ARG RUST_IMAGE=rust:bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6
ARG RUNTIME_IMAGE=ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac
ARG BROWSER_SUPPLIER_IMAGE=node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5
ARG PRODUCT_STAGE=source

FROM ${BROWSER_SUPPLIER_IMAGE} AS browser-supplier
COPY talos/build/container/prepare-browser.sh /build/container/prepare-browser.sh
RUN bash /build/container/prepare-browser.sh

FROM ${BROWSER_SUPPLIER_IMAGE} AS official-mcps
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates && rm -rf /var/lib/apt/lists/*
COPY talos/build/plugins /build/plugins
RUN node /build/plugins/snapshot.mjs /snapshot

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
      -p talos-server -p talos-worker-bin -p talos-linux-sandbox -p talos-tui --release \
    && mkdir -p /artifacts \
    && cp talos/talos-rs/target/release/talos-server talos/talos-rs/target/release/talos-worker \
      talos/talos-rs/target/release/talos-linux-sandbox-launcher talos/talos-rs/target/release/talos /artifacts/

FROM rust-builder AS source
COPY --from=web-builder /workspace/talos/talos-app/web/dist/ /artifacts/webui/

FROM scratch AS packaged
COPY talos/.release-product/bin/ /artifacts/
COPY talos/.release-product/webui/ /artifacts/webui/

FROM ${PRODUCT_STAGE} AS product

FROM ${RUNTIME_IMAGE} AS runtime

ARG VERSION=dev
ARG REVISION=unknown
ARG DEBIAN_FRONTEND=noninteractive

LABEL org.opencontainers.image.title="Talos Server" \
      org.opencontainers.image.description="Talos HTTP API and WebUI server" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.source="https://github.com/apilots/talos" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"

COPY --from=browser-supplier /opt/talos/browser /opt/talos/browser
COPY --from=official-mcps /snapshot /opt/talos/plugins/apilots-mcps
COPY talos/build/worker/browser /opt/talos/browser
COPY talos/talos-rs/crates/action/browser/resources/browser_driver.py /opt/talos/browser/browser_driver.py
COPY talos/talos-app/shared/browser-home /opt/talos/browser/desktop
COPY talos/talos-app/web/public/talos-logo.svg /opt/talos/browser/desktop/talos-logo.svg
COPY talos/build/container/install-runtime.sh /build/container/install-runtime.sh
RUN bash /build/container/install-runtime.sh && rm -rf /build/container
RUN install -d -o talos -g talos /opt/talos/webui

COPY --from=product /artifacts/talos /opt/talos/bin/talos
COPY --from=product /artifacts/talos-server /opt/talos/bin/talos-server
COPY --from=product /artifacts/talos-worker /opt/talos/bin/talos-worker
COPY --from=product /artifacts/talos-linux-sandbox-launcher /opt/talos/bin/talos-linux-sandbox-launcher
COPY --from=product /artifacts/webui/ /opt/talos/webui/
COPY talos/conf/talos-prod.yaml /opt/talos/conf/talos.yaml
COPY talos/conf/migrations /opt/talos/conf/migrations
COPY talos/LICENSE talos/NOTICE talos/THIRD_PARTY_NOTICES /usr/share/licenses/talos/

RUN chmod 755 /opt/talos/bin/talos /opt/talos/bin/talos-server /opt/talos/bin/talos-worker /opt/talos/bin/talos-linux-sandbox-launcher \
  && chown -R talos:talos /home/talos \
  && test -s /usr/share/licenses/talos/LICENSE \
  && test -s /usr/share/licenses/talos/NOTICE \
  && test -s /usr/share/licenses/talos/THIRD_PARTY_NOTICES \
  && /opt/talos/bin/talos-linux-sandbox-launcher --probe-seccomp \
  && bwrap --version \
  && /opt/talos/bin/talos-server --version \
  && /opt/talos/bin/talos-worker --version \
  && /opt/talos/bin/talos --version

ENV HOME=/home/talos \
    LANG=en_US.UTF-8 \
    TERM=xterm-256color \
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
