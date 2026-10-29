# Native browser runtime

This group owns the native Desktop browser engine and Browser MCP integration.
Lightweight Web/Desktop workspace resources live in group 22; Docker Worker and
Server integration lives in group 23. The existing runner and cases are retained.

```bash
just e2e-group 20_browser_workspace
just e2e-case 20_browser_workspace/04_desktop_mcp
```

| Case | Product journey | Assertions |
| --- | --- | --- |
| `03_desktop_browser` | Real Desktop host → bundled browser and explicitly prepared local Chrome | DOM actions, explicit and implicit current-tab close in both browser modes, stale references, separate per-mode screenshot and upload; bundled debugger detach/recovery; both backends deliver different-content same-name downloads without overwrites, never expose an unfinished download, accept UUID names and honor acknowledgement receipts. Only an owned temporary Chrome profile is connected. |
| `04_desktop_mcp` | Actual Desktop controller/app-server/embedded Worker → model task → Browser MCP → File API | Tool discovery and typed dispatch in both bundled and explicitly prepared Chrome modes: browser state/actions, screenshot and attachment authorization, uploads/downloads; pause/resume followed by a real target switch and first navigation without a preparatory tool; disconnect leaves Chrome alive. Local browser capability remains independent of remote display. |

Case 04 prepares the official MCP snapshot from `build/plugins/official-mcp.json`.
Both cases use owned profiles and endpoints, never a user's interactive browser.
The prepared-Chrome case requires port 9222 to be unused. Admission tolerates
TIME_WAIT but rejects an active listener. Native endpoints accept an explicitly
supplied current `TALOS_E2E_APP_SERVER_BIN`.

Use macOS or Linux with Chrome, Node/pnpm, the repository toolchain and Python
fixture dependencies. Linux requires Xvfb and Electron system libraries. Exact
prerequisites remain in requirements.json; do not disable browser sandboxing.
Desktop host and driver unit tests stay with their owners rather than becoming
additional E2E cases.

CI uses directory/group ownership. Desktop native source changes select this
group and the native workspace case in group 22. Media, IPC, controller and
packaging inputs additionally select the Worker container in group 23. Shared
Browser fixtures select their native and container consumers. Each selected case
runs once with current-run executable artifacts.

The controlled-model MCP journey proves integration and lifecycle, not external
provider reasoning quality. Final provider-backed acceptance uses the configured
visual model to browse `https://talos.love`, return source-backed findings and
valid screenshots. Keep that local acceptance separate from public secretless
CI results. Windows, deployed proxy/NAT/TLS and multi-control-plane Sync require
their owning environment acceptance.
