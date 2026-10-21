# Browser and resource workspace

This numbered group owns the Browser MCP and shared resource workspace product
journeys. Run the complete group before handoff:

```bash
just e2e-group 20_browser_workspace
# During development, select one existing numbered case:
talos-e2e/run_e2e.sh --case 20_browser_workspace/01_resource_tabs
```

## Scenarios and observable boundaries

| Case | Product journey | Assertions |
| --- | --- | --- |
| `01_resource_tabs` | Web product layout → two independent local-service previews → local Markdown → close/account switch | Initial/refresh sidebar state does not steal focus, genuine keyboard focus remains visible, and closing restores it; same-URL instances keep separate form state; switching/collapse preserves it; keyboard and modal focus work; close/logout destroys instances; real File API renders Markdown and rejects non-owner metadata/content. |
| `02_desktop_resources` | Actual production Desktop main and renderer → shared preview/Markdown resources | Native IPC, sidebar initial/refresh focus and local File import/render work in the shipped client; same-URL previews retain independent state; local Computer has no Docker desktop entry. This is the native integration slice, not a duplicate Web focus/authorization suite. |
| `03_desktop_browser` | Real Desktop host → bundled browser and explicitly prepared local Chrome | DOM actions, explicit and implicit current-tab close in both browser modes, stale references, separate per-mode screenshot and upload; bundled debugger detach/recovery; both backends deliver different-content same-name downloads without overwrites, never expose an unfinished download, accept UUID names and honor acknowledgement receipts. Only an owned temporary Chrome profile is connected. |
| `04_desktop_mcp` | Actual Desktop controller/app-server/embedded Worker → model task → Browser MCP → File API | Tool discovery and typed dispatch in both bundled and explicitly prepared Chrome modes: browser state/actions, screenshot and attachment authorization, uploads/downloads; pause/resume followed by a real target switch and first navigation without a preparatory tool; disconnect leaves Chrome alive. Local browser capability remains independent of remote display. |
| `05_docker_worker` | Outbound Docker Worker → ordinary task/tools → private live desktop → restart/profile recovery | First non-navigation tool opens a task tab while preserving the private HTTP homepage; navigation/DOM/files and forged attachment rejection; default homepage, background after closing all browser windows and working Browser/Terminal/Files launchers; authenticated media, three consecutive rejected Worker admissions release their candidate reservations while preserving the live controller, input/control fencing, held-key release, backpressure, expiry and viewer teardown; replacement Worker reuses its owned profile. |

Responsive preview checks keep the same address/iframe DOM and drafts. Address
focus survives both directions; leaving a native modal preserves the iframe's
current input focus. Entering a native modal resets a cross-origin child input's
focus in Chromium: the product restores its browsing context, and the user must
refocus the input. The scenario verifies this boundary without reading child DOM
from product code or adding a cross-origin bridge.

Each case crosses a different product boundary. Related assertions run in one
stateful journey rather than replaying a whole model task per assertion. Desktop
host tests and Python driver unit regressions stay with their owners; the group
does not wrap them as additional E2E cases. Screenshots, JSON evidence and process
logs go to the runner's `reports/20_browser_workspace/<case>/` directory.

## Prerequisites

Use macOS or Linux, installed Chrome, Docker, Node/pnpm, the repository Rust
toolchain and protobuf compiler, OpenSSL and Caddy. Install application and Python
dependencies using the repository lockfiles and the Browser fixture requirements,
plus the existing workspace dependencies `grpcio protobuf playwright`. On Linux,
run the group under `xvfb-run -a` with Electron's system libraries and the existing
sandbox prerequisites. Do not disable the browser sandbox. Python Playwright
connects to installed Chrome/Electron; no Playwright browser download is needed.

Set `TALOS_E2E_CHROME_BIN` when Chrome is outside its standard installation path;
`TALOS_E2E_CADDY_BIN` defaults to `caddy`. Port 9222 must be unused for the explicit
prepared-Chrome case; a running user endpoint is rejected, never borrowed.
The fixture isolates Chrome with its own temporary `user-data-dir` while keeping
its normal OS home; Electron receives a separate temporary home. This preserves
platform startup behavior without reading or attaching to a user browser profile.
Endpoint admission tolerates TCP TIME_WAIT but still rejects an active listener. Cases
build current local binaries when a prebuilt binary is not supplied. The shared
Web fixture uses the standard debug Server build. Native cases accept
`TALOS_E2E_APP_SERVER_BIN`; Docker accepts `TALOS_E2E_SERVER_BIN` as well.

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
All profiles, endpoints and containers created by these cases are disposable;
they must not attach to a user's interactive browser or retained review Worker.

## CI and provider-backed acceptance

The secretless PR Browser job builds the standard image and runs this complete
group through `run_e2e.sh`. Lightweight Web preview/Markdown-only edits run case
01 through the same runner without Docker. Heavy Browser owners run the full
group once, without duplicating case 01 in the generic scenario matrix. Missing
dependencies, unsupported runtime or an incomplete fixture result fail explicitly.

Cases 04/05 use a deterministic model endpoint to prove product integration and
permission/lifecycle behavior without provider secrets. They do **not** claim
reasoning quality or autonomous website analysis. Final provider-backed acceptance
uses the configured development visual model and the lowest supported reasoning
effort, submits an ordinary Composer task to browse `https://talos.love`, analyze
product features and return source-backed findings with valid screenshots. Inspect
normal task titles, actual tool calls, recovery errors and rendered final images
separately for local Desktop and Docker Worker; preserve failures instead of
rewriting persisted results. See the Browser fixture README for the reusable
procedure. The same task need not be repurchased for presentation-only changes.

Full multi-control-plane Sync remains group 19 local topology acceptance. Native
Windows support, production proxy/NAT/TLS deployments and user-managed Chrome
permissions require their corresponding environment acceptance; this group does
not label those unavailable environments as passing.
