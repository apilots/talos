# Standalone Worker image

Build from the Talos repository root:

```bash
just docker-worker-build talos-worker:local
```

Cloud authentication and synchronization SDK/types dependencies use their public
Cargo registry versions; no adjacent private source checkout, Sync bundle or
named Docker context is required. The image contains only `talos-worker`, the
existing Linux sandbox launcher, and runtime tools; it contains no Server, WebUI,
target directory or provider secrets. It runs as UID/GID 10001 and uses outbound Connect by default.

Prepare an isolated, writable home with the existing YAML configuration at
`conf/talos.yaml`, the node credential at `conf/node.credential`, and provider
keys in `.env`. Create the user-owned node through the original Computer API and
retain its issued UUID; configure `agent_node.runner` with that same UUID,
`control_plane_endpoint`, `credential_file`, and `transport: outbound`. Production
uses `https://agent.talos.love`; local TLS tests also configure `trusted_ca_file`.
Credential files must be private and readable by the container's chosen UID.

```bash
docker run --name talos-worker \
  --mount type=bind,source=/path/to/worker-home,target=/home/talos/.talos \
  --env TALOS_HOME=/home/talos/.talos \
  talos-worker:local
```

No published port is needed. The original Worker RPC listener stays on protected
loopback; all remote dispatch and resource callbacks use outbound Connect. The
mounted home retains the UUID/credential, Worker history cursor and Workspace
files across container replacement. Pure Workers do not create the management
Session database or connect to the Sync service.

For a local bind mount owned by another non-root UID, pass `--user UID:GID` and
`--env HOME=/home/talos/.talos`; that makes the writable mounted home the directory
picker root too. Optional hardware unavailable inside a Linux container remains
absent; the report never claims access to its physical host's CPU model or GPU.

The public release workflow builds Server and Worker images for linux/amd64 and
linux/arm64 and records both in the release manifest. The current local image is
an acceptance candidate, not a published release. See the independent
[Computer sync E2E](../../talos-e2e/scenarios/19_computer_sync/README.md).
