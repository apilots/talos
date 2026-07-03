# Talos Server Container Release Plan

## 阶段 1: WebUI Resolution
**目标**: Make `talos-server` resolve the packaged WebUI from explicit and installed locations without blocking backend-only startup.

**成功标准**:
- `TALOS_WEBUI_DIR` is supported as an explicit WebUI bundle override.
- Default lookup checks `current_exe_dir/webui`, `current_exe_dir/../webui`, and `current_dir/webui`.
- A valid WebUI directory must contain `index.html`.
- Missing or invalid WebUI logs a warning and starts the HTTP API without WebUI fallback.
- Valid WebUI logs the resolved path and serves static files from that directory.

**测试**:
- Unit tests for explicit `TALOS_WEBUI_DIR`.
- Unit tests for installed `bin/../webui` layout.
- Unit tests for missing WebUI warning behavior.
- Scoped `talos-base` or `talos-gateway-http` checks.

**状态**: 已完成

## 阶段 2: Source-based Server Dockerfile
**目标**: Add a Dockerfile that builds the server image from source and prepares the runtime environment needed by Talos server.

**成功标准**:
- `build/server/Dockerfile` builds WebUI and `talos-server` in separate builder stages.
- Runtime image uses `ubuntu:24.04`.
- Runtime image includes `ca-certificates`, `tzdata`, `tini`, `bash`, `git`, `ripgrep`, `curl`, `libssl3t64`, and `libsqlite3-0`.
- Runtime image runs as non-root user `talos`.
- Runtime image stores persistent data under default `/home/talos/.talos`.
- Runtime image does not set `TALOS_HOME` or `TALOS_RESOURCE_DIR`.
- Runtime image exposes port `8080` and starts `/opt/talos/bin/talos-server` through `tini`.

**测试**:
- Local `docker build -f talos/build/server/Dockerfile -t talos-server:local .` from the shared parent context containing `talos/` and `talos-cloud/`.
- `docker run` starts the server without root.
- Container creates `/home/talos/.talos/conf/talos.yaml` and `/home/talos/.talos/data/talos.db`.

**状态**: 已完成

## 阶段 3: Local Docker Smoke Verification
**目标**: Prove the local image is usable before wiring CI publishing.

**成功标准**:
- Running image responds successfully on `/health`.
- Running image serves WebUI HTML from `/`.
- Backend-only mode remains usable when WebUI is missing or not mounted.
- Container logs clearly identify WebUI serving or API-only startup.

**测试**:
- `curl -fsS http://127.0.0.1:8080/health`
- `curl -fsS http://127.0.0.1:8080/`
- Negative smoke with missing WebUI bundle confirms warning and API availability.

**状态**: 已完成

## 阶段 4: Server Build Target Cleanup
**目标**: Move `./build.sh server` toward the complete server product target while keeping the release workflow stable during migration.

**成功标准**:
- `./build.sh server` produces the server archive with WebUI.
- `server-webui` remains as a temporary alias or compatibility target.
- Docker image construction is gated behind explicit environment variables until release CI is updated.
- Existing tarball artifact names stay compatible with `talos-releases`.

**测试**:
- `PROFILE=release ./build.sh server`
- Archive inspection confirms `bin/talos-server`, `conf/talos.yaml`, and `webui/index.html`.
- Existing release artifact verification script still passes after target migration.

**状态**: 已完成

## 阶段 5: GHCR Multi-architecture Release
**目标**: Update `talos-releases` so public release CI publishes the server image to GitHub Container Registry.

**成功标准**:
- Release workflow has `packages: write`.
- Workflow logs into `ghcr.io` with `GITHUB_TOKEN`.
- `linux/amd64` and `linux/arm64` images are built on native runners.
- Multi-architecture manifest is published as `ghcr.io/apilots/talos-server`.
- Tags include `v<version>`, `<version>`, and stable `latest`.
- Release manifest records image name, tag, and digest.

**测试**:
- `docker buildx imagetools inspect ghcr.io/apilots/talos-server:<tag>` shows both platforms.
- Pull and run `linux/amd64` image smoke.
- Pull and run `linux/arm64` image smoke on arm runner.
- Public GHCR package visibility is verified after first publish.

**状态**: 进行中
