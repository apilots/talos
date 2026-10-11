# Computer workspace acceptance

Run `just e2e-group 18_computer_workspace` with the configured real model providers funded.
The user restored DeepSeek funding and real execution acceptance has resumed;
controlled RPC tests do not replace these scenarios.

| Scenario | Observable acceptance |
| --- | --- |
| `01_registration` | Real device login, protected identity/cache persistence, same UUID after host/control-plane restart, heartbeat/renewal, disable/recovery, owner isolation and logout |
| `02_routing` | Three outbound Workers with no TCP listener; owner routing, pinned Sessions, Worker file effects, live Timeline/history and restart recovery |
| `03_reliability` | Real long-running tools across lease renewal, progress before settlement, concurrent owner activity and local cleanup |
| `04_collaboration` | Real subagent callbacks through the existing control-plane resource RPCs |
| `05_parity` | Existing attachment/vision, compaction, waiting-input restart and embedded behavior through the outbound carrier |
| `06_workspace` | Target defaults/models/directories, Project/Session affinity, real file/Timeline effects, offline and RPC-timeout metadata, unchanged owner errors, changed defaults and removed saved Workspace |
| `08_configuration` | Real target config persistence/publication, per-node admission, runtime Web Search, failed candidate rollback, retained MemorySearch/MemoryGet and compact after Workspace reload, same UUID restart |
| `10_oauth` | Public relay, real SDK PKCE/issuer/code exchange and MCP tool, Worker-only credentials, restart, stale/duplicate callbacks, disable/disconnect, original ten-minute expiry and logout |
| `09_plugins` | Real source/install/update/enable/credentials, DeepSeek MCP tool effects, node isolation, concurrent admission, restart and offline rejection |
| `07_environment` | Two real Workers: same document/date/Memory ID isolation, stale-version rejection, index replacement, Skills operations, real Harness document context, owner/offline rejection and restart preservation |
| `11_product_ui` | Real Electron A/D and Chrome Web: real login button/polling/local UUID, pending first-registration UUID recovery, local/remote file effects and final replies, shared history, second Desktop/Web dispatch to the first Desktop Worker, running-task switch, rename/disable/recovery, legacy embedded Project continuation and collapsed title after reload, narrow Sidebar/Escape/focus, legacy permissions, model deep link, drafts/reload, 22-node pagination, offline history, keyboard/themes/language and real logout/account isolation |

Prerequisites: the adjacent `talos-cloud` checkout (or `TALOS_CLOUD_REPO`), Rust,
Node.js, installed pnpm workspace dependencies, OpenSSL, protoc, Python grpcio,
protobuf and PyYAML. Registration additionally needs Caddy 2.9+ on PATH (or
`TALOS_E2E_CADDY_BIN`); execution needs lsof and a funded provider configured in
`.env`. Product UI requires installed Google Chrome (or `TALOS_E2E_CHROME_BIN`) and
the installed Electron workspace dependency. `TALOS_E2E_PREFERRED_MODELS` selects an existing model. Resource callbacks
may also use the configured inner/vision models; their credentials must work.

`01_registration` starts real Auth, Server, TLS/HTTP2 ingress and two actual
Desktop controllers/SDK/app-server hosts. Device approval uses the real Auth API;
HTTP Computer APIs and node gRPC share the same origin. No fabricated AuthService,
cached token injection or database mutation is used. Cloud Desktop must not
create a product DB. Outbound execution becomes available only after Connect
publication and a healthy heartbeat.

Execution reuses the existing multi-node scenario helpers with outbound node
configuration: Workers never expose a Harness listener and the Server never
connects into them. File observations remain on Worker homes; persistent product
state and Timeline observations remain on the control plane.

Evidence lives under `reports/18_computer_workspace/<scenario>/`. Raw credentials
travel only through in-memory API responses and protected temporary home files,
never the report. SQLite assertions are read-only. Shared setup owns cleanup;
`TALOS_MULTI_NODE_KEEP_TMP=1` retains homes containing test credentials.

To run registration alone: `just e2e-case 18_computer_workspace/01_registration`.
Registration and all execution/environment slices have passed with real services
and DeepSeek. Product UI additionally launches actual Electron and Chrome; its
original 15 user scenarios passed with a Desktop copied from the DMG; later Review repair scenarios are recorded separately in the verification record. Set
`TALOS_E2E_DESKTOP_APP=/path/to/Talos.app/Contents/MacOS/Talos` to use the installed
artifact: the driver removes development binary/resource overrides and asserts
both actual bundled app-server process paths. Without this variable it uses
development Electron and the current app-server binary.

Local smoke uses an isolated copy of DEV YAML with explicit local mode and no
login. Cloud acceptance retains real authentication. Model credentials copied
for local smoke are removed before retaining screenshot evidence.

The topology uses isolated processes/homes on one Mac. Public agent.talos.love,
physical multi-host/network validation and release signing remain unverified;
the local SNAPSHOT package is ad-hoc signed. See the tracked
[verification record](../../../spec/feat-043-computer-workspace/verification.md).

`07_environment` passed its six assertions with real Workers and DeepSeek.
Connect receipt windows also passed ordinary-burst and stalled-consumer regressions;
the existing reliability and direct-routing cases passed again after that change.

The product UI driver clicks the real login button and lets AuthPage poll and save
identity. Approval uses the actual Cloud Auth API. An isolated main-process CDP
observer replaces only Electron's OS browser opener to avoid opening a user's
browser; native auth RPCs and persistence are unchanged. Chrome uses its isolated
profile and actual device-approval tab. The pending-enrollment case creates an
owned node through the API and preserves only its YAML pending barrier to model a
lost response/UUID save; recovery occurs through the actual management page.
