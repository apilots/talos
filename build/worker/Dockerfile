# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e

ARG RUST_IMAGE=rust:bookworm@sha256:114c7a4425406451c2866b6aafe69fe29b1b298832db1277d411ac73c82d04d6
ARG RUNTIME_IMAGE=ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac
ARG BROWSER_SUPPLIER_IMAGE=node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5
ARG PRODUCT_STAGE=source

FROM ${BROWSER_SUPPLIER_IMAGE} AS browser-supplier
ENV PLAYWRIGHT_BROWSERS_PATH=/opt/talos/browser/engines
RUN npm install --prefix /opt/talos/browser/supplier --ignore-scripts playwright@1.64.1 \
    && node /opt/talos/browser/supplier/node_modules/playwright/cli.js install --no-shell chromium \
    && node -e 'const fs=require("fs");const p=require("/opt/talos/browser/supplier/node_modules/playwright");fs.writeFileSync("/opt/talos/browser/runtime.json",JSON.stringify({schemaVersion:1,browserUseVersion:"",chromiumVersion:"156.0.8078.4",python:"/opt/talos/browser/venv/bin/python",driver:"/opt/talos/browser/browser_driver.py",chromium:p.chromium.executablePath()}));' \
    && chmod -R a+rX /opt/talos/browser/engines

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

FROM ${BROWSER_SUPPLIER_IMAGE} AS official-mcps
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates && rm -rf /var/lib/apt/lists/*
COPY talos/build/plugins /build/plugins
RUN node /build/plugins/snapshot.mjs /snapshot

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
    python3 python3-venv xvfb xauth x11-utils openbox x11vnc feh tint2 xterm pcmanfm desktop-file-utils librsvg2-bin fonts-noto-cjk fonts-noto-color-emoji \
    libasound2t64 libatk-bridge2.0-0t64 libatk1.0-0t64 libatspi2.0-0t64 libcairo2 libcups2t64 \
    libdbus-1-3 libdrm2 libgbm1 libglib2.0-0t64 libnspr4 libnss3 libpango-1.0-0 \
    libx11-6 libx11-xcb1 libxcb1 libxcomposite1 libxdamage1 libxext6 libxfixes3 libxkbcommon0 libxrandr2 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=official-mcps /snapshot /opt/talos/plugins/apilots-mcps
RUN python3 -m venv /opt/talos/browser/venv \
    && /opt/talos/browser/venv/bin/pip install --no-cache-dir --require-hashes --only-binary=:all: -r /opt/talos/plugins/apilots-mcps/mcp/browser/requirements.lock
COPY --from=browser-supplier /opt/talos/browser/engines /opt/talos/browser/engines
COPY --from=browser-supplier /opt/talos/browser/runtime.json /opt/talos/browser/runtime.json
COPY talos/build/worker/browser/environment.py /opt/talos/browser/environment.py
RUN /opt/talos/browser/venv/bin/python /opt/talos/browser/environment.py
COPY talos/talos-rs/crates/action/browser/resources/browser_driver.py /opt/talos/browser/browser_driver.py
COPY talos/build/worker/browser/desktop /opt/talos/browser/desktop
COPY talos/talos-app/shared/browser-home /opt/talos/browser/desktop
COPY talos/talos-app/web/public/talos-logo.svg /opt/talos/browser/desktop/talos-logo.svg
RUN rsvg-convert /opt/talos/browser/desktop/wallpaper.svg -o /opt/talos/browser/desktop/wallpaper.png \
    && for icon in browser terminal files; do rsvg-convert -w 64 -h 64 /opt/talos/browser/desktop/$icon.svg -o /opt/talos/browser/desktop/$icon.png; done \
    && cp /opt/talos/browser/desktop/view-file.desktop /usr/share/applications/talos-view-file.desktop \
    && cp /opt/talos/browser/desktop/mimeapps.list /etc/xdg/mimeapps.list \
    && update-desktop-database /usr/share/applications
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
