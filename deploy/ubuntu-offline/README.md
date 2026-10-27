# Ubuntu offline image

Build the committed product source with a private local pre profile:

```bash
bash deploy/ubuntu-offline/build.sh /absolute/path/to/conf/talos-pre.yaml
```

Docker with Buildx, Git and build-time network access are required.
`PLATFORMS="arm64 amd64"`, `BUILD_JOBS=4`, `IMAGE=talos-ubuntu-pre` and
`OUTPUT_DIR=output/ubuntu-server` are configurable. Both targets compile on the
builder's native CPU with a GNU cross compiler for the other architecture.
Image tags and labels record the source revision.

The managed configuration template is current main's `conf/talos-prod.yaml`.
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

Copy the matching offline bundle to the server and run (`arm64` on ARM):

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

Run the actual packaged profile checks and the existing formal Server case06:

```bash
bash deploy/ubuntu-offline/smoke.sh talos-ubuntu-pre:<revision>-arm64
bash deploy/ubuntu-offline/smoke.sh talos-ubuntu-pre:<revision>-amd64
NETWORK=bridge MODEL_TEST=1 bash deploy/ubuntu-offline/smoke.sh talos-ubuntu-pre:<revision>-arm64
bash deploy/ubuntu-offline/browser-smoke.sh talos-ubuntu-pre:<revision>-arm64
```

Smoke checks use host Python 3. Case06 additionally needs host PyYAML and
websockets; it uses a local deterministic model in a network-isolated container
and covers real desktop pixels/input, release/disconnect recovery, Browser MCP,
TUI deletion keys and model persistence. Actual evidence stays in ignored
`output/` and `talos-e2e/reports/`; no duplicate Browser fixture is maintained.
Private YAML stays out of Git and is embedded only in the local image.
