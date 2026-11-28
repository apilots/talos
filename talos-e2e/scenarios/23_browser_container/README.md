# Packaged Browser integration

This group owns Docker Worker and Server browser/media integration. It is
separate from lightweight workspace tabs (22) and native Browser/MCP (20).

```bash
just e2e-group 23_browser_container
just e2e-case 23_browser_container/01_docker_worker
just e2e-case 23_browser_container/03_managed_talo
just e2e-case 23_browser_container/04_authenticated_worker_owner
```

| Case | Product journey | Assertions |
| --- | --- | --- |
| `01_docker_worker` | Outbound Docker Worker → ordinary task/tools → private live desktop → restart/profile recovery | First non-navigation tool opens a task tab while preserving the private HTTP homepage; navigation/DOM/files and forged attachment rejection; default homepage, background after closing all browser windows and working Browser/Terminal/Files launchers; authenticated media, three consecutive rejected Worker admissions release their candidate reservations while preserving the live controller, input/control fencing, held-key release, backpressure, expiry and viewer teardown; replacement Worker reuses its owned profile. |
| `02_docker_server` | Packaged anonymous Server → shared CLI/PTY TUI → model edit/add → restart → desktop viewing/control → embedded Browser MCP | Real RFB pixels and X11 key state prove anonymous viewing and input; release clears held input and permits an Agent task; disconnect suspends automation, clears input, and explicit resume permits another task. Each task performs seven real Browser actions and a File API screenshot. Model edits persist across a Server process restart; the TUI exercises DEL and Ctrl-H. Docker networking is disabled and model/page fixtures remain local. |
| `03_managed_talo` | Authenticated Server with a local Docker socket → managed Talo Worker → lifecycle | Uses a disposable local registry for the Worker image digest. An ordinary Max user initializes Talo from the local Electron renderer and sees a populated framebuffer. The native conversation uploads an image, selects it from the Server file list, and sends it. The fixture releases the final reply chunk after the first chunk renders. Reloading restores the reply and its image attachment. A separately signed-in Chrome browser opens the same Server conversation and displays the message sent from Desktop and its reply. The native workspace viewer renders a Server-only PDF and a text file from the execution Worker. The personal Files page and Home retain their personal Computer selection and exclude the Talo file store. The dedicated Talo Console Files tab lists and opens the same Server-only PDF. An explicitly targeted first Turn creates an additional Server-owned Talo conversation; its direct native route and reload display one persisted Turn while preserving the personal Computer selection. The user submits a Goal from Talo and a Loop from a fresh native profile opened directly to the same conversation. Both resolve the protected Worker Skill. The controlled model completes the Goal and stops the Loop after the submitting Desktop exits, and a separate authenticated client verifies the Server’s persisted outcomes. Verifies Worker registration over TLS, account-scoped ownership, initialization retry, manual stop and start, stale generation rejection, Home retention across Reset, idle sleep, explicit Wake, a first Turn with Worker-local model configuration, the same live Desktop media and control case as the personal Worker, and cleanup after confirmed Delete. |
| `04_authenticated_worker_owner` | Two device sign-ins → registered embedded Worker → Web and cold native Desktop App Server | The administrator lists, pauses, runs and resumes Built-in jobs through both clients. Web renders live pixels and native Desktop receives RFB frames after starting with an empty directory and Sync disabled. The periodic Server pull discovers the owner Computer; Desktop rename immediately updates list/detail, and a direct Server rename appears through the next directory refresh. A standard user is denied the administrator's Worker, can run their Desktop host's existing jobs locally, and retains exclusive control of those jobs. |

Case 04 runs in the hosted Server matrix for PR and main-branch changes to
Worker registration and routing, authentication, Built-in jobs, browser/media,
their Web and Desktop entry points and SDKs, packaged Server inputs, and its
fixtures. Shared infrastructure changes use the existing full selection.
Documentation, unrelated UI, and unrelated TUI changes leave the case unselected.
Mixed changes select one Server job. The runner requires a current packaged
Server image, the App Server binary, Chrome, Caddy, and the declared Python
and Node dependencies. The hosted job prepares those inputs before preflight.
The disposable issuer supplies device approvals for two fixed test identities.
The product clients complete the normal login flow and Server authorization.
This case exercises the Desktop Controller and App Server media bridge.

Run case 03 with `TALOS_E2E_TALO_RECOVERY=1` to exercise a disposable TLS
relay fault. The recovery path covers command replay without Wake, a queued
Turn that survives delayed registration, both real Wake deadlines, and offline
Reset retention. For local native Electron
acceptance, pass `TALOS_WEBUI_DIR` and an executable
`TALOS_E2E_TALO_DESKTOP_PROBE`. The case invokes the probe with the Server URL,
Computer ID, report directory and stage before and after Reset.


Case 01 checks the installed source commit, dependency lock and actual browser-use
version, reuses matching preinstallation across replacement and unchanged updates,
and preserves disabled state across restart through the original Plugin API. In
its disposable container, removing packaged environment metadata triggers exact-lock
online preparation; another real Browser task must then succeed with the prepared
environment. Persisted manual pause is resumed explicitly through the public API.
The cold native client uses the same TLS ingress for product HTTP/media and
Worker registration, without copying the Server database. This requires Caddy.
The native account/directory fixture also runs the real `talos plugin` CLI against
the outbound Docker Worker for both Skill and MCP lifecycle operations; no remote
installation may appear in the client home. The same case's production Desktop
IPC/preload/SDK and noVNC viewer must also take control, apply a key to the real
X11 display, clear held input on release and disconnect, then resume with actual
pixels and another Agent Browser task. It shares the HTTP viewer's X11 probe;
connection status alone is not rendered-frame evidence. The native fixture records
host send, ACK acceptance, request completion and stream release as they happen
in `native-media-frames.jsonl`, so records already written survive a fixture
timeout. Match request IDs to renderer handler entry and duration in
`native-docker-viewer-diagnostics.json` (or the failure diagnostics). Compare
send and handler timestamps for IPC delivery, handler duration for consumption,
and ACK/completion/release timestamps for response and cleanup. Wall-clock gaps
include process scheduling. After online repair, a replacement
container removes its packaged environment metadata before Worker startup and
disables external package DNS while retaining the control-plane hosts entry. It
must reuse the same home environment and complete another real Browser task.

For focused outbound Worker package acceptance, run
`TALOS_E2E_PLUGIN_PACKAGES_ONLY=1 just e2e-case 23_browser_container/01_docker_worker`
with a current-source Worker image. This runs the real CLI, Server and Docker
Worker path without starting Electron or calling a model. It checks Worker-local
ZIP and MCP JSON paths, wrong-kind rejection before Marketplace publication,
disabled state across replacement, and installation isolation from the client.
The full case also runs these package checks.

Build the **current source** Worker image using the standard build context (the
parent directory must contain this checkout as `talos/`):

```bash
docker build -f talos/build/worker/Dockerfile -t talos-worker:browser-e2e .
```

Alternatively, the release/CI packaged stage copies current Linux Worker and
sandbox-launcher binaries into `.release-product/bin/` and builds the same
Dockerfile with `--build-arg PRODUCT_STAGE=packaged`. Set
`TALOS_E2E_WORKER_IMAGE` for another explicitly built current-source tag. The Docker
case fails if the image is missing; it does not pull an unrelated published image.
Case 02 uses the current standard Server image, built with
`docker build -f talos/build/server/Dockerfile -t talos-server:browser-e2e .`,
or selected through `TALOS_E2E_SERVER_IMAGE`. Its packaged stage consumes the same
current-run binaries plus `talos-server`, the `talos` CLI and production WebUI
under `.release-product/webui/`. CI builds these inputs once and uses
`PRODUCT_STAGE=packaged` without recompiling Rust inside Docker. This case needs
only Docker, Python, PyYAML and the existing fixture's pinned `websockets` package
on the host. The case mounts that Python package read-only for its RFB viewer;
no test dependency is added to the production image. The image supplies the
Browser runtime and pinned official MCP delivery. It validates a Server process
restart with the same home and the embedded local media route, independently of
the registered Worker's authenticated Cloud media route. Actual pixels and
state records are grouped under `scenarios/01-view`, `02-takeover`,
`03-release-agent` and `04-disconnect-recover` in the case report directory.
Case 03 uses both current images and a temporary registry on loopback. The Server
runs as its image's non-root user with the Unix socket's supplemental group.
The Worker reaches the test Agent endpoint from its own Docker bridge. The
case requires a local Linux Docker Engine (including a Linux VM on macOS) and
uses the existing mock account service plus a per-run TLS certificate.

All profiles, endpoints and containers created by these cases are disposable;
they must not attach to a user's interactive browser or retained review Worker.


CI derives Worker/Desktop versus Server execution from the existing image
requirements. Changes under this scenario directory select the group; shared
Browser fixture changes select groups 20 and 23. Product Worker/Server owners
retain their respective container acceptance. These scenarios use deterministic
loopback model/page fixtures and real product/runtime boundaries. Provider-backed
reasoning quality remains separate local acceptance.

### Authenticated directory acceptance

`04_authenticated_worker_owner` starts a real isolated Server and cold native
Desktop hosts with separate databases and Sync disabled. It waits for the
Server directory pull before selecting a remote Computer, checks immediate
rename write-through and a subsequent Server-side rename, then exercises the
existing Worker control and media path. The Web topology selects the exact
`Inspect <display name>` button before `Desktop`; it must not use a broad
Talos text match that can select the control-plane card instead.
