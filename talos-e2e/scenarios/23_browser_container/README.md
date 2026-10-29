# Packaged Browser integration

This group owns Docker Worker and Server browser/media integration. It is
separate from lightweight workspace tabs (22) and native Browser/MCP (20).

```bash
just e2e-group 23_browser_container
just e2e-case 23_browser_container/01_docker_worker
```

| Case | Product journey | Assertions |
| --- | --- | --- |
| `01_docker_worker` | Outbound Docker Worker → ordinary task/tools → private live desktop → restart/profile recovery | First non-navigation tool opens a task tab while preserving the private HTTP homepage; navigation/DOM/files and forged attachment rejection; default homepage, background after closing all browser windows and working Browser/Terminal/Files launchers; authenticated media, three consecutive rejected Worker admissions release their candidate reservations while preserving the live controller, input/control fencing, held-key release, backpressure, expiry and viewer teardown; replacement Worker reuses its owned profile. |
| `02_docker_server` | Packaged anonymous Server → shared CLI/PTY TUI → model edit/add → restart → desktop viewing/control → embedded Browser MCP | Real RFB pixels and X11 key state prove anonymous viewing and input; release clears held input and permits an Agent task; disconnect suspends automation, clears input, and explicit resume permits another task. Each task performs seven real Browser actions and a File API screenshot. Model edits persist across a Server process restart; the TUI exercises DEL and Ctrl-H. Docker networking is disabled and model/page fixtures remain local. |

Case 01 checks the installed source commit, dependency lock and actual browser-use
version, reuses matching preinstallation across replacement and unchanged updates,
and preserves disabled state across restart through the original Plugin API. In
its disposable container, removing packaged environment metadata triggers exact-lock
online preparation; another real Browser task must then succeed with the prepared
environment. Persisted manual pause is resumed explicitly through the public API.
The native account/directory fixture also runs the real `talos plugin` CLI against
the outbound Docker Worker for both Skill and MCP lifecycle operations; no remote
installation may appear in the client home. The same case's production Desktop
IPC/preload/SDK and noVNC viewer must also take control, apply a key to the real
X11 display, clear held input on release and disconnect, then resume with actual
pixels and another Agent Browser task. It shares the HTTP viewer's X11 probe;
connection status alone is not rendered-frame evidence. After online repair, a replacement
container removes its packaged environment metadata before Worker startup and
disables external package DNS while retaining the control-plane hosts entry. It
must reuse the same home environment and complete another real Browser task.

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
All profiles, endpoints and containers created by these cases are disposable;
they must not attach to a user's interactive browser or retained review Worker.


CI derives Worker/Desktop versus Server execution from the existing image
requirements. Changes under this scenario directory select the group; shared
Browser fixture changes select groups 20 and 23. Product Worker/Server owners
retain their respective container acceptance. These scenarios use deterministic
loopback model/page fixtures and real product/runtime boundaries. Provider-backed
reasoning quality remains separate local acceptance.
