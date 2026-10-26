# Standalone Worker image

Build from the Talos repository root:

```bash
just docker-worker-build talos-worker:local
```

Cloud authentication and synchronization SDK/types dependencies use their public
Cargo registry versions; no adjacent private source checkout, Sync bundle or
named Docker context is required. The image contains only `talos-worker`, the
existing Linux sandbox launcher, built-in Browser MCP, managed Chromium and its
Browser Use driver, Xvfb/Openbox/x11vnc, tint2, xterm, PCManFM, fonts and runtime
tools; it contains no Server, WebUI, build target directory or provider secrets.
It runs as UID/GID 10001 and uses outbound Connect by default.

Prepare an isolated, writable home with the existing YAML configuration at
`conf/talos.yaml`, the node credential at `conf/node.credential`, and provider
keys in `.env`. Create the user-owned node through the original Computer API and
retain its issued UUID; configure `agent_node.runner` with that same UUID,
`control_plane_endpoint`, `credential_file`, and `transport: outbound`. Production
uses `https://agent.talos.love`; local TLS tests also configure `trusted_ca_file`.
Credential files must be private and readable by the container's chosen UID.
Managed default configuration is upgraded from the bundled template before load,
with a backup. Existing UUIDs, credentials and recovery state are retained.
Explicit `TALOS_CONFIG` files remain operator-owned and are not normalized.
Server and Worker images share `conf/talos-prod.yaml` as the packaged template;
the Worker entrypoint starts only execution and registration capabilities.

```bash
docker run --name talos-worker \
  --mount type=bind,source=/path/to/worker-home,target=/home/talos/.talos \
  --env TALOS_HOME=/home/talos/.talos \
  --shm-size=1g --security-opt seccomp=deploy/docker/talos-worker/chromium-seccomp.json \
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

## Managed browser boundary

The standard Worker image includes its browser dependencies in both source and
packaged product builds. It uses Ubuntu 24.04 and Chromium supplied by Node
Playwright 1.64.1 (Chrome for Testing 156.0.8078.4). Browser's manifest and complete
Python dependency lock come from the official `apilots/mcps` commit recorded in
`build/plugins/official-mcp.json`. The same source snapshot is packaged for Desktop;
Desktop uses its native backend and does not install Python. Update the official
repository lock first, then deliberately advance this build input after acceptance.
The Node supplier remains pinned by image digest.

Image builds install upstream wheels using exact versions and hashes and generate
Browser-owned environment metadata from the installed packages. Worker startup
verifies lock/platform/ABI/API and actual versions; a matching environment is reused
without a package download. Missing environments are prepared outside execution and
Plugin commit gates. A dependency-changing update after binding fences Browser until
that Worker restarts. Optional Browser repair failure leaves the Worker available.
No package manager runs when a task first invokes Browser.

Browser resources start lazily under UID/GID 10001. Chromium keeps its own
profile and downloads under the protected Worker home. Xvfb is a virtual X server;
Openbox supplies the lightweight window manager. Startup waits for its readiness
before preparing the application windows. The existing cool wallpaper palette carries a restrained central Talos emblem
and fine orbital accents, with no promotional text. A compact tint2 dock provides
three real application launchers:

| Entry | Local application and behavior |
| --- | --- |
| Browser | The existing managed Chromium opens its packaged offline home. Closing the last window preserves the supervised process/CDP; reopening uses the same profile and login state. |
| Terminal | xterm opens a real container shell as UID/GID 10001, initially in the Worker's `browser/workspace` directory. Commands act with that container user's existing permissions. |
| File manager | PCManFM opens that same directory as UID/GID 10001 with `--no-desktop`, without taking over the desktop or adding a host file-sharing service. Text files open in a read-only xterm preview; no separate editor is required. |

All three applications use the same private X display. Terminal and File manager
start on user demand; their launchers are local GUI actions, not new MCP tools,
Browser targets, terminal APIs or file-transfer services. Human input still needs
the existing authenticated desktop takeover. Read-only viewers cannot click or
type. Returning control revokes further input but does not undo commands or file
operations already performed, and closing a viewer does not close application
windows. Worker shutdown reaps the managed display and its application processes.
Only files in existing persistent volumes/workspaces survive replacement under
their original mount semantics; the container's writable layer is not durable.
The launchers' default directory is `browser/workspace` under the protected Worker
home (normally `/home/talos/.talos/browser/workspace`), so it uses that home's
existing persistence and ownership rather than a new store or mount.
The original Turn/File API authorization remains the model tool boundary.

This is a small container desktop, without a full desktop environment, login
manager or Dashboard service. It does not capture a physical host screen.
CDP uses an ephemeral loopback port, RFB uses loopback 5900, and X11
TCP is disabled. The image does not expose these ports or include a
websockify/noVNC HTTP server. noVNC belongs to the product Web client and reaches
the Worker only through the authenticated media relay. Native Desktop and Server
packages do not gain these container desktop resources.

The static home lives in `talos-app/shared/browser-home/` and is also packaged by
the native Desktop browser build. Its assets have no credentials or runtime
authority. ManagedBrowser supervises a private HTTP child bound to an ephemeral
`127.0.0.1` port within its existing startup and cleanup lifecycle. This child
preloads only `home.html`, `home.js` and `talos-logo.svg`, accepts GET/HEAD for
those exact paths, and exposes no directories, arbitrary files or writes. Initial
Chromium, its homepage setting and dock reopening share this trusted URL. This
keeps the home compatible with Browser Use without bypassing its file security
policy or adding a published port. Native Desktop still loads the packaged file.
The Docker wallpaper, dock and local application entries live in
`build/worker/browser/desktop/`. Native Desktop's bundled browser keeps its local
toolbar, and user Chrome keeps its original UI; neither gains these Linux apps
or a native screen-sharing service.

Use the supplied Compose security profile and private 1 GiB shared memory. Do
not use privileged mode, host networking/display mounts, `--no-sandbox` or
`seccomp=unconfined` to repair sandbox startup. Namespace sandbox support must be
available on the container host; a failed browser startup leaves the Worker
manageable and reports the browser failure separately.

The deployment profile is derived from
[Moby's default seccomp profile](https://github.com/moby/profiles/blob/main/seccomp/default.json),
upstream SHA256 `6416b47770785a41ac59073cdc77d9fe98517df2799dc83ef207e622de3053f6`,
with only `clone`, `setns` and `unshare` allowed for Chromium user namespaces.
Keep the profile beside the Compose file. Recheck it against the host runtime
when updating Chromium or the container runtime; retain the Chromium sandbox
inspection and no-public-listener acceptance gates from feat-047.

### Media ingress and reverse proxies

The existing trusted Agent TLS ingress must route
`/talos.agent.v1.AgentDesktopMedia/Reserve`, `/Worker` and `/Viewer` as HTTP/2
gRPC, alongside the original Agent services. Worker streams authenticate with
their existing node lease; viewer/reservation streams use the current account.
Do not add a direct route to the container's CDP or VNC listeners.

The product HTTPS origin must forward `/v1/desktop/open` and WebSocket upgrades
for `/v1/desktop/stream`. Preserve the original Host and Origin, disable response
buffering for the stream, and allow long-lived bidirectional connections. A
gateway moved behind a proxy still requires the browser's exact host/port
Origin; do not synthesize an allowed Origin at the proxy. Configure trusted CA
files through the existing Agent client configuration, never disable TLS checks.
Desktop clients use their existing protected preload/stdio last mile and the same
trusted Agent media service; they do not need a localhost HTTP service.

Tickets expire after 30 seconds and are single-use. Web submits the ticket and
current access token only in its first message, within five seconds; neither
belongs in a URL or proxy access log. Admission allows two viewers per Computer
and one controller. Chunks are at most 64 KiB, each application queue holds at
most 64 chunks, and two seconds of blocked consumption terminates a stream.
Additional transport/OS buffers remain bounded separately. A slow spectator
does not share the task/heartbeat queue. Closing a controlling stream preserves
the explicit pause until the user resumes or returns control.

Plan bandwidth from the actual workload: one uncompressed 1440×900, 32-bit frame
is 5,184,000 bytes before transport overhead. RFB compression and change tracking
reduce typical traffic but do not guarantee a video frame rate or fixed cost.
Validate private-CA, HTTP/2, WSS upgrades, disconnects and slow viewers through the
deployment's real proxy chain using the reusable
[browser acceptance scenario](../../talos-e2e/scripts/browser-worker/README.md).
