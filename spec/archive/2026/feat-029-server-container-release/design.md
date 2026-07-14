# Talos Server Container Release Design

Status: archived
Archived: 2026-07-14
Owner: agent
Created: 2026-07-03
Updated: 2026-07-03

## Background

Talos currently publishes the server product as a `Talos-Server-<version>-<platform>-<arch>.tar.gz` archive. The archive is produced by `build.sh server-webui` and contains:

- `bin/talos-server`
- `conf/talos.yaml`
- `webui/`
- `start.sh`

The public `apilots/talos-releases` repository owns the external release workflow. It is triggered by tags from the private Talos repository, checks out the private source, builds release artifacts, and publishes them as public release assets.

The next release surface is a Docker image for the Talos HTTP server and WebUI. The image must be suitable for GitHub Container Registry and support `linux/amd64` and `linux/arm64`.

## Current Behavior

### Server archive

The existing server archive is built on the host runner. This works for downloadable tarballs, but it is not a reliable Docker image input for multi-architecture images:

- A host-built tarball can contain a binary for the host platform, not the image platform.
- A macOS tarball cannot be copied into a Linux image and expected to run.
- A multi-architecture image needs each image variant to contain the binary compiled for that target platform.

### WebUI discovery

`talos-server` currently only checks:

```text
current_exe_dir/webui/index.html
```

If the WebUI bundle is not found, the server silently starts without WebUI fallback. This keeps the backend available, but makes release packaging errors hard to notice.

The current archive layout puts `webui/` next to `bin/`, so a server launched from `bin/talos-server` should also check:

```text
current_exe_dir/../webui/index.html
```

## Goals

1. Add a source-based Dockerfile for the server image.
2. Keep Talos runtime data and configuration under the default `~/.talos` path.
3. Do not set `TALOS_HOME` or `TALOS_RESOURCE_DIR` in the image.
4. Support explicit WebUI bundle override with `TALOS_WEBUI_DIR`.
5. Start the HTTP API even when the WebUI bundle is missing.
6. Log a warning when the WebUI bundle cannot be resolved.
7. Support local Docker image validation before release CI is changed.
8. Let `talos-releases` publish multi-architecture images to GHCR.

## Non-goals

- Do not build the Docker image from a host-generated server tarball.
- Do not require WebUI to exist for backend-only local debugging.
- Do not add Node, Playwright, or browser dependencies to the default runtime image.
- Do not move Talos persistent config, data, logs, or workspace files outside `~/.talos`.
- Do not introduce a second runtime configuration root for Docker.
- Do not make the private Talos repository publish GHCR packages directly.

## Product Boundary

The server image is the deployable HTTP server product:

```text
talos-server
  HTTP API
  SSE events
  WebUI static bundle when present
  local SQLite state under ~/.talos
  Agent tools that depend on basic userland commands
```

The image is not a full browser automation image. MCP browser support can be added later as a separate image flavor if needed.

## WebUI Resolution

Add a dedicated WebUI resolver in `talos-base`.

Resolution order:

1. `TALOS_WEBUI_DIR`
2. `current_exe_dir/webui`
3. `current_exe_dir/../webui`
4. `current_dir/webui`

A candidate is valid only when:

```text
<candidate>/index.html
```

exists as a file.

### Missing WebUI behavior

Missing WebUI must not prevent server startup.

If `TALOS_WEBUI_DIR` is set but invalid:

```text
TALOS_WEBUI_DIR is set but <path>/index.html was not found; starting API-only server.
```

If no default candidate is valid:

```text
Talos WebUI bundle not found; starting API-only server. Set TALOS_WEBUI_DIR to serve a WebUI bundle.
```

If a candidate is valid:

```text
Serving Talos WebUI from <path>
```

This keeps local backend-only debugging usable while still exposing packaging mistakes in logs.

## Dockerfile Design

Add:

```text
build/server/Dockerfile
```

The Dockerfile builds from source and must not consume `Talos-Server-*.tar.gz`.

### Build context

The build context must contain both repositories:

```text
talos/
talos-cloud/
```

This is required because the Rust workspace currently depends on:

```text
../../talos-cloud/cloud-rs/crates/cloud-sdk-rs
```

Local build context:

```text
/Users/xujiang/data/ailabs
```

CI build context should be normalized to:

```text
docker-context/
  talos/
  talos-cloud/
```

### Stages

#### web-builder

Base image:

```text
node:24-bookworm
```

Responsibilities:

- Install pnpm through Corepack or the repository-approved package manager setup.
- Install `talos-app` dependencies with the lockfile.
- Build `talos-web`.

Output:

```text
talos/talos-app/web/dist/
```

#### rust-builder

Base image:

```text
rust:bookworm
```

Build dependencies:

- `pkg-config`
- `libssl-dev`
- `libsqlite3-dev`

Responsibilities:

- Build `talos-server` in release mode.

Command:

```text
cargo build --manifest-path talos/talos-rs/Cargo.toml -p talos-server --bin talos-server --release
```

Output:

```text
talos/talos-rs/target/release/talos-server
```

#### runtime

Base image:

```text
ubuntu:24.04
```

Runtime dependencies:

- `ca-certificates`
- `tzdata`
- `tini`
- `bash`
- `git`
- `ripgrep`
- `curl`
- `libssl3t64`
- `libsqlite3-0`

Runtime layout:

```text
/opt/talos/bin/talos-server
/opt/talos/conf/talos.yaml
/opt/talos/webui/
/home/talos/.talos/
```

Runtime user:

```text
user: talos
home: /home/talos
```

Environment:

```text
HOME=/home/talos
SHELL=/bin/bash
PATH=/opt/talos/bin:...
```

The image must not set:

```text
TALOS_HOME
TALOS_RESOURCE_DIR
```

Container metadata:

```text
EXPOSE 8080
VOLUME ["/home/talos/.talos"]
ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/opt/talos/bin/talos-server"]
```

Healthcheck:

```text
curl -fsS http://127.0.0.1:8080/health
```

## Local Validation

Build from the shared parent directory:

```bash
cd /Users/xujiang/data/ailabs
docker build \
  -f talos/build/server/Dockerfile \
  -t talos-server:local \
  .
```

Run:

```bash
docker run --rm \
  -p 8080:8080 \
  -v talos-home:/home/talos/.talos \
  --name talos-server-local \
  talos-server:local
```

Verify:

```bash
curl -fsS http://127.0.0.1:8080/health
curl -fsS http://127.0.0.1:8080/
```

Expected results:

- `/health` returns healthy JSON.
- `/` returns WebUI HTML when the bundle is present.
- `~/.talos/conf/talos.yaml`, `~/.talos/data/talos.db`, `~/.talos/logs/`, and workspace directories are created under `/home/talos/.talos`.

## Build Script Direction

`./build.sh server` should become the primary server product target.

Target behavior:

1. Build the server archive with WebUI.
2. Optionally build a Docker image when explicitly requested by environment.

Implemented environment contract:

```text
SERVER_IMAGE=1
SERVER_IMAGE_NAME=talos-server
SERVER_IMAGE_TAGS=<version>
SERVER_IMAGE_PLATFORMS=linux/amd64,linux/arm64
SERVER_IMAGE_PUSH=0
DOCKER_BUILD_CONTEXT=<shared parent containing talos/ and talos-cloud/>
```

Docker image construction is opt-in through `SERVER_IMAGE=1` so the existing tarball release path remains stable while `talos-releases` CI is updated.

`server-webui` can remain as a temporary alias to avoid breaking the existing release workflow during migration.

## Release CI Design

The public `talos-releases` workflow remains the publisher.

Required permission:

```yaml
permissions:
  contents: write
  packages: write
```

GHCR login:

```yaml
- uses: docker/login-action@v3
  with:
    registry: ghcr.io
    username: ${{ github.actor }}
    password: ${{ secrets.GITHUB_TOKEN }}
```

### Multi-architecture build

Use native runners instead of QEMU for the first implementation:

| Platform | Runner |
| --- | --- |
| `linux/amd64` | `ubuntu-latest` |
| `linux/arm64` | `ubuntu-24.04-arm` |

Each runner builds and pushes an image by digest. A final publish job creates the multi-architecture manifest with:

```text
docker buildx imagetools create
```

Image name:

```text
ghcr.io/apilots/talos-server
```

Tags:

```text
v<version>
<version>
latest
```

`latest` should only be moved by stable release tags.

Required OCI labels:

```text
org.opencontainers.image.title=Talos Server
org.opencontainers.image.source=https://github.com/apilots/talos-releases
org.opencontainers.image.version=<version>
org.opencontainers.image.revision=<source_sha>
```

## Release Manifest

The release manifest should eventually include image metadata:

```json
{
  "images": [
    {
      "name": "ghcr.io/apilots/talos-server",
      "tag": "v0.1.3",
      "digest": "sha256:..."
    }
  ]
}
```

This lets release consumers verify the published image without scraping GHCR.

## Validation Matrix

### Talos repository

- WebUI resolver unit tests.
- Dockerfile local build.
- Docker run smoke test for `/health`.
- Docker run smoke test for `/`.
- Backend-only smoke test with missing WebUI warning.

### talos-releases repository

- GHCR login succeeds with `GITHUB_TOKEN`.
- `linux/amd64` image builds and runs.
- `linux/arm64` image builds and runs.
- Multi-architecture manifest contains both platforms.
- Release manifest records image digest.

## Rollout Plan

1. Implement WebUI resolver warning behavior in Talos.
2. Add source-based server Dockerfile.
3. Validate local Docker build and run.
4. Move `build.sh server` toward the full server product target.
5. Update `talos-releases` CI with GHCR permissions and multi-architecture image jobs.
6. Add image metadata to the public release manifest.
