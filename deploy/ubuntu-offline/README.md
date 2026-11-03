# Ubuntu offline deployment

Use `build.sh` to create the offline package and the packaged `run.sh` to deploy
or upgrade it. This branch maintains deployment scripts; product changes come
from `main`.

## Sync main and build

Run from the repository root of the `feat/ubuntu-offline-image` worktree.
The build machine needs Docker with Buildx, Git, Python 3.11+ and network access.

```bash
git fetch origin
git merge --no-edit origin/main
git push origin feat/ubuntu-offline-image

PLATFORMS=amd64 bash deploy/ubuntu-offline/build.sh
```

Resolve any merge conflicts before continuing. Images use committed `HEAD`, so
commit intended changes before building. The script automatically builds the
image, exports it and creates the delivery archive and checksums under
`output/ubuntu-offline/`:

- `talos-ubuntu-pre-amd64-offline.tar`: the complete package to copy to the server.
- `SHA256SUMS`: checksums for the generated files.
- `bundle-manifest.json`: source revision and package metadata.

Use `PLATFORMS=arm64` for ARM64. Omit `PLATFORMS` to build both architectures and
also generate `talos-ubuntu-pre-<revision>-dual-arch-offline.tar`.
Set `OUTPUT_DIR` to keep separate output directories for different builds.

## Extract on the server

The following commands use the AMD64 package. The server needs Docker and
Chromium user namespace support; startup does not download packages.

```bash
mkdir -p talos-offline
tar -xf talos-ubuntu-pre-amd64-offline.tar -C talos-offline
cd talos-offline
sha256sum -c SHA256SUMS-amd64
```

## First deployment

```bash
sudo bash ./run.sh amd64
```

`run.sh` imports the bundled image when needed and starts container `talos-pre`
with data volume `talos-pre-data`. The port mapping is **host 19091 → container
8080**. Open `http://<server-ip>:19091`.

## Upgrade an existing deployment

Extract the new package as above, then run these commands from its directory:

```bash
sudo docker rm -f talos-pre
sudo env DATA_VOLUME=talos-pre-data HTTP_PORT=19091 bash ./run.sh amd64
```

Use the **existing** container and volume names if they differ from these
defaults; set `CONTAINER_NAME` on `run.sh` for a different container name.
Remove only the old container, **never the data volume**.

The same volume retains `conf/talos.yaml`, the database and workspace under
`/home/talos/.talos`. Do not overwrite the saved `talos.yaml` with a template.
At startup, Talos upgrades its structure while preserving valid custom settings.
The image contains only the source revision's `conf/talos-prod.yaml` upgrade
template; private deployment YAML, model endpoint overrides and credentials are
not build inputs. New volumes use production defaults, including authentication
enabled.

The upgrade keeps **19091 → 8080**. To open the TUI:

```bash
sudo docker exec -it talos-pre talos
```
