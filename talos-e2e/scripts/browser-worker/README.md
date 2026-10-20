# Browser Worker acceptance

This secretless scenario uses current product binaries, the standard Worker image,
a deterministic local model and synthetic account authentication. It does not use
funded models, saved user profiles, production credentials or external accounts.

Run the standard numbered `20_browser_workspace` group; see its
[scenario and prerequisite contract](../../scenarios/20_browser_workspace/README.md).
The individual helpers below are implementation details of its numbered cases,
not a second acceptance runner.

Build the Server, Worker and app-server from the same source. Build the source
Worker image with `build/worker/Dockerfile`, or copy matching Linux Worker and
sandbox binaries into `.release-product/bin` and build with `PRODUCT_STAGE=packaged`.
The Docker context must contain the repository as `talos/`, as in the Worker build
README. Install `requirements.txt` in a test Python environment.

For native coverage, install the repository's existing frontend dependencies and
Electron development runtime, run `pnpm --dir talos-app/desktop build:main`, then
`node talos-e2e/scripts/browser-worker/build-native.mjs`. Native fixtures use only
owned disposable profiles and the existing product preload, controller, stdio and
browser-host modules. Linux runs require Xvfb and normal Electron sandbox support.

Run the built engine fixture with Electron (`.tmp/browser-worker/native/dist`) to
verify the shared offline home, isolated native toolbar, real tab
open/focus/close, stale tab/document rejection, explicit debugger detach failure
and reset recovery. Website views must have no toolbar/product preload, and the
toolbar must be absent from automation targets. Closing a background tab must
preserve the current tab; closing the local browser window hides it, and reopening
retains the dedicated profile. Prepared user Chrome keeps its own UI.

Run `native_mcp.py` with `TALOS_E2E_APP_SERVER_BIN` pointing to the current
app-server. It executes a second real MCP task after revision-bound pause/resume
and requires fresh successful observations.
Run `fixture.py` with `TALOS_E2E_SERVER_BIN`, `TALOS_E2E_APP_SERVER_BIN`, and
`TALOS_E2E_WORKER_IMAGE`; set `TALOS_E2E_NATIVE_MEDIA=1` to require the actual Desktop
noVNC viewer. GitHub PR coverage requires this native media path. The controlled
Docker scenario must cover these desktop behaviors through actual authenticated
RFB input, not just invoke launcher commands directly:

- Make the first Browser MCP call a non-navigation tab-open operation. Keep the
  original private HTTP homepage as a background tab while navigating the new
  task tab; verify both actual target identities and URLs remain observable.
- Wait for Openbox readiness, then observe the neutral wallpaper and compact
  Browser, Terminal and File manager dock. The cool wallpaper has a restrained
  central Talos accent without promotional text.
- Observe the packaged offline home and live clock; close every browser window
  while retaining the same CDP process/profile and an updating RFB connection.
  Reopen through the real dock and supported Openbox browser shortcut.
- Open all three real applications from their dock entries. Terminal must be
  xterm and File manager must be PCManFM; verify their UID/GID 10001 and private
  display environment. Use real Terminal input to create a disposable text file
  in the default Worker-home `browser/workspace`; observe it in File manager and
  open its read-only text preview. Close and reopen the applications without
  breaking the browser/media path. Remove only fixture-owned files afterward.
- Keep read-only input rejection, current-controller fencing and no new public
  listener checks. These local applications must not add MCP tools or Browser
  targets. Save empty-desktop and opened-application frames in ignored output.

The same fixture distinguishes the finite title and memory auxiliary requests:
title returns a normal task name, memory returns its JSON no-op, and unknown
auxiliary workloads fail explicitly. After completion it checks the generated
title through both Session detail and list APIs. Run the focused HTTP contracts
with `python -m unittest discover -s talos-e2e/scripts/browser-worker -p '*_test.py'`.
These tests also execute the packaged Python adapter against partial, failing
and hanging Browser Use sessions. Startup and stop share one cleanup owner;
their cooperative budgets reuse the dependency's 15-second CDP connection and
Worker's 5-second cleanup limits. Cancellation-resistant dependencies remain
bounded by the existing outer 120-second Worker/process deadline. Cleanup failure
must fail the operation, even if its browser action already completed.

The same fixture also asserts completion markers, actual browser results, original File API persistence,
attachment denial, no Worker port mapping, loopback-only listeners, private-CA
outbound media, read-only enforcement, controller epochs, held-input cleanup, and rejection of a stale same-viewer control
replacement without changing the existing controller or disconnecting its RFB socket.
Evidence uses the standard scenario report directory (`TALOS_SCENARIO_REPORT_DIR`);
direct helper runs default to ignored `output/browser-worker/`; task-owned containers
and volumes are removed in `finally`.

The Desktop media fixture seeds a directory snapshot from the synthetic owning
Gateway database. Coordinator attachment, private-CA G/H routing, NodeAuth media,
Desktop IPC and noVNC rendering remain real. This represents the synced directory
precondition, not an end-to-end account Sync transport test. Full multi-machine
Sync/NAT/proxy deployments and user-installed Chrome compatibility remain local
acceptance scenarios; they do not replace the hosted representative product gate.

The existing PR scope selector runs the Browser image/native job for its Web
owners, SDK/Desktop, backend, dependencies, configuration and build/CI changes.
Unrelated Web-only pages/assets retain the smaller existing representative gate;
expected skips are validated explicitly, never treated as a failed Browser run.
Shared home resources, native chrome resources and the shared browser logo are
build inputs for this gate, even when the logo lives under Web public assets.

## Visual and real-model acceptance

The deterministic hosted fixture does not replace final product visual checks.
Exercise the original Session/Computer entry, MCP enable-only row, natural remote
screen proportions, nearby centered controls, narrow/expanded views, focus return,
takeover feedback and all three dock entries. Check native bundled-browser home
and toolbar separately from prepared Chrome, whose original UI remains unchanged.
Record observed results/screenshots and any unavailable checks only under ignored
`output/browser-worker/`; tracked specs describe acceptance requirements, not runs.

If the user authorizes a real-provider website analysis, run it separately and
last, after local regression checks, independent review repairs and final visual
acceptance. Follow the real-model selection contract in
`spec/feat-047-browser-computer-desktop/design.md`, section 11: choose the configured
image-capable DeepSeek model and the lowest supported reasoning setting in the
normal product model selector (`deepseek-v4-flash-vision-exp`, `light`; verify its
provider mapping is `low`). Record both the execution Worker's selection and the
management Server's independent `agent.inner_model` selection; changing the
Session model does not change the auxiliary title/memory model. Keep synthetic
and real-provider evidence separate. Read and scroll the public `talos.love`
website, then inspect actual model-generated
Browser calls, screenshot evidence, source links and the final feature analysis.
Verify the completed task has a normal title in Session detail, list and topbar.
Preserve historical polluted Sessions: there is no title-edit/regenerate public
API, and clearing history to reset its title would destroy the task evidence.
Use a new task after fixing its auxiliary-model precondition.
Do not substitute the deterministic model's canned response or mark this step
passed before execution. Keep provider credentials out of logs, events and
screenshots; preserve only redacted evidence in ignored output. This optional
authorized run is not part of the secretless GitHub fixture above.
