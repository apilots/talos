# Ubuntu offline image

This branch owns the entire offline image workflow in `deploy/ubuntu-offline/`:

| Entry | Responsibility |
| --- | --- |
| `Dockerfile` | Compile Server, Worker, TUI and WebUI; install runtime resources |
| `build.sh` | Build/export images from this checkout's committed `HEAD`, then package them |
| `package.py` | Create portable per-architecture and combined delivery bundles |
| `run.sh` | Select the host architecture, import the image and start its persistent container |
| `smoke.sh`, `browser-smoke.sh` | Exercise the actual profile and formal Browser container scenario |
| `verify-bundle.py` | Check a delivery bundle, import its images and verify launcher/persistence |

Run from this repository root. Commit intended source/build changes on
`feat/ubuntu-offline-image` before building with a private local pre profile:

```bash
bash deploy/ubuntu-offline/build.sh /absolute/path/to/conf/talos-pre.yaml
```

Docker with Buildx, Git, Python 3.11+ and build-time network access are required.
`PLATFORMS="arm64 amd64"`, `BUILD_JOBS=4`, `IMAGE=talos-ubuntu-pre` and
`OUTPUT_DIR=output/ubuntu-server` are configurable. Both targets compile on the
builder's native CPU with a GNU cross compiler for the other architecture.
Image tags and labels record the source revision.

The default build exports both architectures and creates:

- `talos-ubuntu-pre-arm64-offline.tar` and `talos-ubuntu-pre-amd64-offline.tar`;
- `talos-ubuntu-pre-<revision>-dual-arch-offline.tar`, with both images and one launcher;
- `SHA256SUMS` and `bundle-manifest.json`, covering only this invocation's targets.

Each delivery contains `source-revision.txt`, checksum files and usage instructions.
Single-target builds create only that target's bundle; use a fresh `OUTPUT_DIR`
for each build so historical artifacts remain separate. Image archives, private
profiles, logs and actual acceptance reports stay outside Git. Reusable build
and verification code belongs here, rather than under ignored `output/` paths.

The managed configuration template is this source revision's `conf/talos-prod.yaml`.
The private pre profile seeds `/home/talos/.talos/conf/talos.yaml` in a new data
volume; only `gateway.host` is set to `0.0.0.0` for container access. The existing
Talos startup upgrader normalizes configuration against the current template.
Packaging does not rename retired fields or edit plugin installation state.
Existing volumes keep their saved configuration and use that same startup path.
The pre profile supplies anonymous access, port 19091 and disabled Sync.

The image contains Server, WebUI, embedded Worker, TUI, migrations and the sandbox
helper. Browser resources and the pinned official MCP snapshot use the formal
`build/container` and `build/plugins` installation flow. Browser automation,
desktop viewing and takeover work for the anonymous embedded Worker without
Cloud login. Registered remote Workers retain their own admission requirements.
Server and TUI share the named volume's configuration, database and workspace.

Preinstalled tools: kubectl v1.37.1 (official target-architecture SHA256 verified),
SSH/SCP/SFTP, cron/crontab, curl/wget, dig/nslookup, ip/ss, ping/traceroute/nc,
ps/top/pgrep, lsof/strace/tcpdump, jq, less/vi, rsync, compression tools, OpenSSL,
Git and ripgrep. Cron is installed but not automatically started. Kubeconfig and
SSH credentials are operator mounts. Model requests use the pre profile's service
addresses; startup and the packaged Browser need no public package downloads.

Copy the combined bundle to the server and replace `REVISION` below with the
eight-character revision in its filename. Extract it into its own directory:

```bash
tar -xf talos-ubuntu-pre-REVISION-dual-arch-offline.tar
sha256sum -c SHA256SUMS-images
sudo bash ./run.sh
```

Alternatively, copy the matching single-architecture bundle (`arm64` on ARM):

```bash
ARCH=amd64
tar -xf "talos-ubuntu-pre-${ARCH}-offline.tar"
sha256sum -c "SHA256SUMS-${ARCH}"
sudo docker load -i "talos-ubuntu-pre-${ARCH}.tar.gz"
sudo bash ./run.sh "$ARCH"
sudo docker exec -it talos-pre talos
```

Open `http://<server-ip>:19091`. The outer `.tar` is a delivery bundle; Docker loads
the inner `.tar.gz`. The bundle includes the matching Chromium seccomp profile,
image tag metadata and launcher. `CONTAINER_NAME=talos-pre`, `HTTP_PORT=19091` and
`DATA_VOLUME=talos-pre-data` are the launcher defaults. Stop/remove the old
container before upgrading, and reuse its data volume. Do not remove the volume.

The launcher uses the formal Chromium seccomp profile, `no-new-privileges` and
1 GiB shared memory. The host must support Chromium's user namespaces. No
privileged mode or sandbox-disabling flags are used. The local smoke probe marks
seccomp UNAVAILABLE when cross-architecture emulation returns kernel EINVAL;
that kernel behavior must be verified on native target hardware.

Run the actual packaged profile checks and the existing formal Server container case:

```bash
bash deploy/ubuntu-offline/smoke.sh talos-ubuntu-pre:<revision>-arm64
bash deploy/ubuntu-offline/smoke.sh talos-ubuntu-pre:<revision>-amd64
NETWORK=bridge MODEL_TEST=1 bash deploy/ubuntu-offline/smoke.sh talos-ubuntu-pre:<revision>-arm64
bash deploy/ubuntu-offline/browser-smoke.sh talos-ubuntu-pre:<revision>-arm64
```

Smoke checks use host Python 3. The formal container case additionally needs host
PyYAML and websockets; it uses a local deterministic model in a network-isolated
container and covers real desktop pixels/input, release/disconnect recovery, Browser MCP,
TUI deletion keys and model persistence. Actual evidence stays in ignored
`output/` and `talos-e2e/reports/`; no duplicate Browser fixture is maintained.
Private YAML stays out of Git and is embedded only in the local image.

Check the delivery archive through its own launcher, including image revision,
anonymous HTTP/WebUI, operator tools and retained-volume container recreation:

```bash
python3 deploy/ubuntu-offline/verify-bundle.py \
  output/ubuntu-server/talos-ubuntu-pre-REVISION-dual-arch-offline.tar \
  --output-dir output/ubuntu-server/bundle-verification
python3 -m unittest discover -s deploy/ubuntu-offline/tests -v
```

The verifier accepts either delivery format and tests every architecture in the
archive. It allocates temporary containers/volumes and removes them afterward;
imported image tags remain available. This verifies delivery and startup, while
the smoke entry points own runtime feature acceptance.

To repackage existing image exports without rebuilding product binaries, keep
their original launcher/seccomp assets and `image-prefix.txt`, then supply the
full revision that actually built those images:

```bash
python3 deploy/ubuntu-offline/package.py /absolute/path/to/export-directory \
  FULL_ORIGINAL_SOURCE_REVISION arm64 amd64
```

Packaging rejects an image-prefix/revision mismatch and records the original
source revision independently of the checkout's current `HEAD`.
