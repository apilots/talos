# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
ARG BROWSER_SUPPLIER_IMAGE=node:24-bookworm-slim@sha256:a5f4366e834695d5f363b920c104953a535d3a9864d1bdf74566a0ebe01daee5
ARG RUST_IMAGE=rust:1.99-bookworm@sha256:48b7f435b8fc6378ce43b44f15e3a47fd8919b2825f835c39bd4bd3da50af5b8
ARG NODE_IMAGE=node:24.21.0-bookworm@sha256:3d27e5c11e5786e309ec3e03f93ae536eb36e6e5eb3714d5eb3300a36157add0
ARG RUNTIME_IMAGE=ubuntu:24.04@sha256:68ee22004a9109b44973313b02ff7b1de250f1de0f5ecb628d73f579aaff33ac

FROM --platform=$BUILDPLATFORM ${NODE_IMAGE} AS web-builder
WORKDIR /workspace
RUN corepack enable && corepack prepare pnpm@9.14.4 --activate
COPY talos-app ./talos-app
RUN pnpm --dir talos-app install --frozen-lockfile \
    && pnpm --dir talos-app --filter talos-web build

FROM ${BROWSER_SUPPLIER_IMAGE} AS browser-supplier
COPY build/container/prepare-browser.sh /build/container/prepare-browser.sh
RUN bash /build/container/prepare-browser.sh

FROM --platform=$BUILDPLATFORM ${BROWSER_SUPPLIER_IMAGE} AS official-mcps
RUN apt-get update && apt-get install -y --no-install-recommends git ca-certificates && rm -rf /var/lib/apt/lists/*
COPY build/plugins /build/plugins
RUN node /build/plugins/snapshot.mjs /snapshot

FROM --platform=$BUILDPLATFORM ${RUST_IMAGE} AS rust-builder
ARG TARGETARCH
ARG BUILDARCH
ARG BUILD_JOBS=4
WORKDIR /workspace
RUN set -eu; \
    case "$TARGETARCH" in \
      amd64) triple=x86_64-linux-gnu; rust_target=x86_64-unknown-linux-gnu; compiler_package=x86-64-linux-gnu ;; \
      arm64) triple=aarch64-linux-gnu; rust_target=aarch64-unknown-linux-gnu; compiler_package=aarch64-linux-gnu ;; \
      *) exit 1 ;; \
    esac; \
    dpkg --add-architecture "$TARGETARCH"; \
    apt-get update; \
    apt-get install -y --no-install-recommends ca-certificates pkg-config protobuf-compiler \
      "libssl-dev:$TARGETARCH" "libsqlite3-dev:$TARGETARCH"; \
    if [ "$BUILDARCH" != "$TARGETARCH" ]; then \
      apt-get install -y --no-install-recommends "g++-$compiler_package"; \
      rustup target add "$rust_target"; \
    fi; \
    rm -rf /var/lib/apt/lists/*
COPY talos-rs ./talos-rs
RUN --mount=type=cache,id=talos-pre-target-${TARGETARCH},target=/workspace/talos-rs/target \
    --mount=type=cache,id=talos-pre-cargo-registry,target=/usr/local/cargo/registry,sharing=locked \
    set -eu; \
    case "$TARGETARCH" in \
      amd64) triple=x86_64-linux-gnu; rust_target=x86_64-unknown-linux-gnu ;; \
      arm64) triple=aarch64-linux-gnu; rust_target=aarch64-unknown-linux-gnu ;; \
    esac; \
    if [ "$BUILDARCH" != "$TARGETARCH" ]; then \
      case "$TARGETARCH" in \
        amd64) export CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER="$triple-gcc" \
          CC_x86_64_unknown_linux_gnu="$triple-gcc" CXX_x86_64_unknown_linux_gnu="$triple-g++" \
          AR_x86_64_unknown_linux_gnu="$triple-ar" ;; \
        arm64) export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$triple-gcc" \
          CC_aarch64_unknown_linux_gnu="$triple-gcc" CXX_aarch64_unknown_linux_gnu="$triple-g++" \
          AR_aarch64_unknown_linux_gnu="$triple-ar" ;; \
      esac; \
      export PKG_CONFIG_ALLOW_CROSS=1 PKG_CONFIG_LIBDIR="/usr/lib/$triple/pkgconfig:/usr/share/pkgconfig"; \
    fi; \
    cargo build --locked --manifest-path talos-rs/Cargo.toml --release --target "$rust_target" \
      -j "$BUILD_JOBS" -p talos-server -p talos-worker-bin -p talos-linux-sandbox -p talos-tui; \
    mkdir /artifacts; \
    cp "talos-rs/target/$rust_target/release/talos-server" \
      "talos-rs/target/$rust_target/release/talos-worker" \
      "talos-rs/target/$rust_target/release/talos-linux-sandbox-launcher" \
      "talos-rs/target/$rust_target/release/talos" /artifacts/

FROM ${RUNTIME_IMAGE} AS browser-runtime
COPY --from=browser-supplier /opt/talos/browser /opt/talos/browser
COPY --from=official-mcps /snapshot /opt/talos/plugins/apilots-mcps
COPY build/worker/browser /opt/talos/browser
COPY talos-rs/crates/action/browser/resources/browser_driver.py /opt/talos/browser/browser_driver.py
COPY talos-app/shared/browser-home /opt/talos/browser/desktop
COPY talos-app/web/public/talos-logo.svg /opt/talos/browser/desktop/talos-logo.svg
COPY build/container/install-runtime.sh /build/container/install-runtime.sh
RUN bash /build/container/install-runtime.sh && rm -rf /build/container
FROM browser-runtime AS runtime
ARG TARGETARCH
ARG KUBECTL_VERSION=v1.37.1
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      cron strace tcpdump python3-yaml \
    && rm -rf /var/lib/apt/lists/*
# Download the target-architecture client and verify the official checksum.
RUN set -eu; \
    curl -fsSL --retry 3 "https://dl.k8s.io/release/${KUBECTL_VERSION}/bin/linux/${TARGETARCH}/kubectl" -o /tmp/kubectl; \
    curl -fsSL --retry 3 "https://dl.k8s.io/release/${KUBECTL_VERSION}/bin/linux/${TARGETARCH}/kubectl.sha256" -o /tmp/kubectl.sha256; \
    cd /tmp; printf '%s  kubectl\n' "$(cat kubectl.sha256)" | sha256sum --check; \
    install -m 0755 kubectl /usr/local/bin/kubectl; rm kubectl kubectl.sha256
COPY --from=rust-builder /artifacts/ /opt/talos/bin/
COPY --from=web-builder /workspace/talos-app/web/dist/ /opt/talos/webui/
COPY conf/talos-prod.yaml /opt/talos/conf/talos.yaml
COPY conf/migrations /opt/talos/conf/migrations
COPY --chown=talos:talos pre.yaml /home/talos/.talos/conf/talos.yaml
RUN python3 - <<'PYTHON'
from pathlib import Path
import yaml
path = Path('/home/talos/.talos/conf/talos.yaml')
profile = yaml.safe_load(path.read_text())
profile['gateway']['host'] = '0.0.0.0'
path.write_text(yaml.safe_dump(profile, sort_keys=False))
PYTHON
COPY LICENSE NOTICE THIRD_PARTY_NOTICES /usr/share/licenses/talos/
ARG REVISION
LABEL org.opencontainers.image.title="Talos Ubuntu Pre" \
      org.opencontainers.image.description="Anonymous offline Server, WebUI and shared TUI" \
      org.opencontainers.image.source="https://github.com/apilots/talos" \
      org.opencontainers.image.revision="${REVISION}" \
      org.opencontainers.image.licenses="Apache-2.0"
ENV HOME=/home/talos \
    TALOS_HOME=/home/talos/.talos \
    TALOS_RESOURCE_DIR=/opt/talos \
    TALOS_TUI_NO_BROWSER=1 \
    SHELL=/bin/bash \
    LANG=en_US.UTF-8 \
    TERM=xterm-256color \
    PATH=/opt/talos/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
USER talos
WORKDIR /home/talos/.talos
EXPOSE 19091
VOLUME ["/home/talos/.talos"]
HEALTHCHECK --interval=10s --timeout=5s --start-period=30s --retries=6 \
    CMD curl -fsS http://127.0.0.1:19091/health >/dev/null || exit 1
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["talos-server"]
