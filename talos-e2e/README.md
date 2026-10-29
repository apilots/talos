# Talos end-to-end tests

[Chinese](README_zh.md)

`talos-e2e` validates real product flows across the Rust runtime, gateways,
terminal client, Web/Desktop adapters, SDKs, external services, packaging, and
installation. Unit tests stay with their owning packages; this workspace proves
cross-component behavior.

## Layout

```text
talos-e2e/
|-- scenarios/        Executable scenarios grouped by product flow
|-- scripts/          Shared runner, server, fixture, and assertion helpers
|-- tests/            Runner and helper regression tests
|-- docker/           Fixed container environments
|-- reports/          Generated reports and logs
`-- run_e2e.sh        Scenario discovery and execution entry point
```

## Run

From the repository root:

```bash
just e2e --list                     # Review the complete discovered scope
just e2e --check                    # Check prerequisites; no builds or cases
just e2e                            # Run all discovered scenarios
just e2e-group 01_models
just e2e-case 01_models/health_and_models
just e2e-group 09_app_server
just e2e-group 14_sandbox
```

Direct runner usage is also supported:

```bash
talos-e2e/run_e2e.sh --case 09_app_server/protocol_e2e
talos-e2e/run_e2e.sh --group 15_tui_install
```

Default, `--case` and `--group` use one sorted filesystem catalog: numbered
`NN_name.sh` files at `scenarios/` root and top-level `*.sh` cases inside numbered
`NN_name/` directories. Nested helpers and unnumbered directories are excluded.
New cases and numbered groups are discovered without updating the runner. Exact
relative paths take precedence over basename lookup; ambiguous basenames fail.
Selectors cannot be combined or repeated. Optional `.sh` suffixes remain supported.

`--list` can accompany a selector and only prints its selected relative paths. It
does not load `.env`, probe dependencies, create reports, build or run anything.
`--check` uses the same scope and loads the normal `.env`; it checks prerequisites
without building, starting services or calling models. Missing prerequisites are
reported together with affected cases in `reports/preflight_*.log` and block the
whole selected run before any build/case. Docker cases are never silently omitted.
A prerequisite check does not prove credentials, network reachability, browser
policy or runtime behavior; original scenario checks remain authoritative.

Dependencies live beside their cases in `requirements.json`. `default` applies to
that directory; `cases` provides filename-specific field replacements, including
empty lists for secretless/platform-independent exceptions. Root legacy cases use
`scenarios/requirements.json`; group declarations do not inherit that root file.
The declaration is not a case registry: new files inherit their group defaults,
and a new group must declare its dependencies before execution. Unknown fields,
stale overrides and missing declarations fail explicitly. Supported fields are
`commands`, `python_modules`, `checks`, `platforms`, `env`, and `binaries`.
`checks` covers concrete existing prerequisites: Docker daemon, Cloud fixture,
real model key via `has_real_llm_key`, installed Chrome, Linux Desktop display,
Caddy, current Worker image availability and native sandbox dependencies.
`binaries` declares the shared `server`, `cli`, `worker`, `app_server` and
`test_client`/`sandbox` prebuild; scenario-owned specialized
builds stay with their existing wrappers. The Linux launcher is prepared only for selected native binary/sandbox cases;
container-only cases do not build a host launcher.
Environment values and secrets are never printed. Example:

```json
{"default":{"commands":["cargo","protoc"],"checks":["model"],"binaries":["cli"]},
 "cases":{"catalog.sh":{"checks":[]}}}
```

Use `TALOS_CLOUD_SERVER_BIN`, `TALOS_E2E_CHROME_BIN`, `TALOS_E2E_CADDY_BIN`,
`TALOS_E2E_WORKER_IMAGE`, and the owning group's documented prerequisites as needed.
Group19 requires native macOS and the pinned public Sync release; it remains local
acceptance, so a default full run on an unsupported platform is blocked explicitly.
Default full runs pass each case's group identity, matching `--group` semantics:
native sandbox cases are not implicitly repeated in Docker before the dedicated
Docker case. An explicitly selected standalone sandbox case retains its existing
macOS-plus-Docker behavior.

Prefer a single case during development and the owning group before handoff.
Runner regressions are lightweight and do not execute product scenarios:

```bash
python3 -m unittest discover -s talos-e2e/tests -p '*_test.py' -v
```

Python3.9+ is required for discovery. `--help` requires only Bash.

## Scenario ownership

| Groups | Scope |
| --- | --- |
| `01_models` | Secretless server health, model catalog, custom Provider persistence and protocol tool loops |
| Root `02`-`03` cases | Session lifecycle, HTTP, and SSE |
| `05`-`08` | CLI, tools, memory, and embedded TUI |
| `09` | App Server protocol, Skills, MCP, and Memory integration |
| `11`-`13` | Cloud auth, files, and durable user interaction |
| `14` | Unified sandbox and permission enforcement |
| `15` | Release bundle installation and installed TUI startup |
| `17` | Three Workers: public registration, owner isolation, routing, lease renewal, collaboration and Workspace ownership |
| `18` | Computer workspace: real Desktop device login, durable UUID, outbound registration and renewal |
| `19` | Multi-control-plane sync: two native Mac Desktop runtimes, native and Docker Workers, Docker WSS hub, real auth and model tasks |
| `20` | Native Desktop browser engine and Browser MCP |
| `21` | Registered/session file previews and real document-format rendering |
| `22` | Lightweight Web/Desktop workspace tabs, local HTTP and Markdown |
| `23` | Docker Worker/Server browser, media/control and profile recovery |

Inspect `scenarios/` for the current case list; it is the source of truth.
Groups `04` and `10` are not assigned; use the scenario directory rather than assuming
that group numbers are contiguous. Frontend unit tests belong to the frontend
quality workflow and are not repeated as an E2E scenario.

The GitHub PR/main coordinator selects the union of affected module checks.
Named WebUI, Desktop, TUI and Server (client/SDK/skills/MCP/tools/runtime) workflows
share one change detector and current-run binary build. WebUI and Desktop wrappers
share `ci-category.yml`, with module-specific quality and E2E plans.
Server/shared-runtime acceptance retains `01_models`, public `09_app_server`
smoke/recovery, Computer deletion, `12_files`, `14_sandbox`, `06_tools/output_logs`
and relevant native Browser/Worker cases. Owning Rust package tests include the
controlled DeepSeek transport regressions without provider credentials. Leaf
modules select relevant packages and representative scenarios rather than every
Server suite. See [CI ownership and reuse](../docs/development/DEVELOPMENT.md#validation-by-delivery-stage).

`08_tui/controlled_chat` is the secretless TUI gate. It reuses the controlled
protocol upstream with the real terminal, Agent and Bash tool; verifies streamed
completion and persisted tool/answer events; resizes the window and exits through
`/quit`. Existing model-backed TUI/interaction groups remain available to
maintainers. Run it with `just e2e-case 08_tui/controlled_chat`.

`15_tui_install` runs in release and manual `release-preflight.yml` only. Native
Linux AMD64/ARM64 and macOS ARM64 acceptance installs the product already built
on that runner. Linux Docker acceptance requires `TUI_INSTALL_PRODUCT_DIR` pointing
to that run's product directory; it never compiles a replacement. Container failure
blocks image publication and the GitHub Release. PR/main select only lightweight
installation contracts, while related Docker Worker and platform business E2E
remain module gates. PR jobs receive no repository secrets and check out no private
repositories. Successful exact-input task/build reuse is distinct from Cargo's
dependency cache, with provenance reported in each reused job summary.

`01_models/protocol_compatibility` runs the real Gateway and Agent against one
controlled loopback upstream. Chat, Responses and Messages use an identical opaque
model ID under distinct Providers. Each performs an On-to-Off session with actual
Bash execution and tool-result replay; Chat covers all four `reasoning_replay`
policies. The case checks protocol-native authentication, root-level KV projection,
reasoning continuation. It requires no
provider credentials or external model network. The Agent path uses upstream
streaming; owning `ProtocolProvider` Rust transport tests cover both streaming and
non-streaming tool exchanges. Auxiliary title calls do not count as non-streaming
Agent tool coverage. Fixture request records and event logs are retained with the
normal scenario evidence.

An additional Messages connection keeps explicit adaptive thinking while changing
effort from medium to high in one session. Both tool-loop rounds must send the
selected `output_config.effort`, retain thinking KV siblings and preserve signed
history. This contract does not imply Chat/Responses endpoints accept adaptive.

Maintainers can manually run the provider-backed `trusted-e2e.yml` workflow from
`main`, including the full `09_app_server` group whose execution, MCP, memory and
skills cases need real model credentials. The public app-server smoke case reuses
the existing client to exercise model listing, Session creation, configuration and
Workspace/Memory API reads without executing a model. Scenarios `11_auth`,
`17_multi_node`, and `18_computer_workspace` still
use the configured Cloud fixture executable for isolated account setup.
`11_auth/cloud_gateway` uses real device approval and personal Computer registration
before restoring App-server/CLI sessions. It checks the owned model catalog and
offline Auth rejection; no provider completion is required.
`09_app_server/computer_deletion` runs real Chrome, HTTP Gateway/Service/Store and
node gRPC with an explicitly selected secretless upstream Auth contract fixture.
It covers owner isolation, offline inspection, disable/enable, cancel/confirm,
English/Chinese prompts, clearing selection and rejected old credentials. The
same case covers the homepage after selection is cleared, selecting an offline
Computer, the zero-registration prompt, and the connection guide with Escape
focus recovery on wide and narrow layouts. A 25-Computer directory also verifies
internal list scrolling, server search beyond the initially loaded results, and
clearing a search; the homepage has no pagination controls. Run
`talos-e2e/scripts/computer_workspace/deletion.py` without `--contract-auth` for
local acceptance with real disposable Cloud Auth (`TALOS_CLOUD_SERVER_BIN`). Both
modes require Python Playwright and installed Chrome (`TALOS_E2E_CHROME_BIN`).
This case does not exercise a multi-manager Sync deployment; that topology
remains local group 19 acceptance, with deletion/history covered separately by
owning-package two-peer Store/CRDT tests.

`22_workspace_resources/01_resource_tabs` mounts the actual product layout in an isolated
system Chrome instance against real Server/File APIs. Only upstream Auth is a
contract fixture. Deterministic local previews cover independent same-URL tabs,
retained form state across switching/collapse, keyboard selection, native modal
and Escape focus, close/account-scope destruction, and authenticated Markdown
rendering with non-owner metadata/content rejection. It also checks the compact
workspace opening width, retained drag width and transparent expanded/narrow
backdrops. Web-only changes select this case, Computer deletion, and the shared
renderer's Desktop resource integration (`22/02`). Desktop-only source/resource
changes select `22/02` and native Browser/MCP group 20. Media, preload, IPC, controller and
packaging inputs also select Docker Worker case `23/01` with the current-run Worker image.
Shared SDK/runtime/build inputs expand to affected consumers; mixed changes take
the union and execute each selected case once. WebUI and Desktop use separate
named wrappers over the common module workflow. See the numbered
[native Browser group 20](scenarios/20_browser_workspace/README.md),
[workspace group 22](scenarios/22_workspace_resources/README.md),
[container group 23](scenarios/23_browser_container/README.md), and
[development validation](../docs/development/DEVELOPMENT.md#validation-by-delivery-stage).
Client scenario/helper directories select their owning groups. CI uses the existing
requirements declarations to place native-display and image consumers; case names
do not encode execution categories. Workspace helpers also select group 21 because
it reuses their harness. LibreOffice and CJK fonts are installed only for scenarios
that declare `soffice`, rather than every Web/Desktop job. Unknown shared
infrastructure remains conservative.

Computer live media teardown is covered separately by the Browser Worker
acceptance and the owning ComputerDesktop component tests; this lightweight case
does not claim a live remote desktop connection.

`09_app_server/desktop_recovery` uses the real controller, SDK, Server and
app-server processes with controlled loopback Auth/model HTTP fixtures. It checks
actual credential enforcement, SIGTERM replacement, exact delivered/history event
parity, unchanged Computer/Session, a subsequent controlled turn and the original
unsubscribe handle. It also verifies Cloud-offline local metadata, complete History,
status, Live subscriptions and idle deletion without additional Cloud requests.
This gated scenario replaces the former opt-in real-model Desktop stdio restart
test; renderer/SDK forwarding remains covered by the controller unit suite.
It needs no private source or provider credentials. Native
packaged Desktop plus Server/Web continuation remains a local acceptance using
real Auth and Sync; controlled fixtures do not replace that product verification.
The larger `19_computer_sync` topology also remains local acceptance.
Cross-manager continuation acceptance must send a real model/tool task to its
pinned Worker, continue the same Session from the receiving manager once its
terminal history is visible, and verify the exact file effect only on that Worker.
Do not manually change execution markers or require a pre-test Shared wait to
hide the admission race. A local Linux container plus a physical Mac exercises
cross-OS execution; report it separately from acceptance on two physical machines. These
exclusions are explicit and do not claim that the corresponding scenarios passed
in GitHub CI.

Computer topology adds representative hosted coverage through the Rust quality
gate (`talos-sync` HTTPS/auth/retention-independent client checks and the
protocol/SDK tests) and frontend component/SDK/IPC checks. The committed Metrics v1
snapshot is checked without a sibling checkout. Real multi-management-host
presence, HTTP/RPC account isolation and same-Worker continuation are also checked
locally by the `talos-base` feature `e2e` scenario with an explicit `TALOS_SYNC_BIN`;
that larger cross-project topology is not claimed as hosted CI acceptance. See
[Computer topology](../spec/feat-045-computer-topology/design.md).

This PR subset keeps representative gated behavior while preventing fork code
from receiving secrets. Provider-backed and Cloud-fixture acceptance is
maintainer-owned; it does not run automatically for each outside contribution.

## Environment requirements

| Scope | Additional requirement |
| --- | --- |
| Base server, CLI, and TUI | Rust toolchain and repository test tools |
| Web and Desktop | Node.js, pnpm dependencies, and platform UI prerequisites |
| Real model or MCP flows | Network access and the provider credentials required by the scenario preflight; use the maintainer workflow or local acceptance |
| Cloud auth | Cloud fixture executable (`TALOS_CLOUD_SERVER_BIN` or `cloud-server` on `PATH`), explicit test credentials, and isolated resources |
| S3 files | curl with SigV4 support; pinned Versity Gateway v1.8.0 (downloaded and checksum-verified by group 12) |
| Linux sandbox supplements | Docker on a supported host |
| Release installation | Native target environment; Docker supplements Linux only |

Each scenario owns its preflight and reports missing variables or tools. Read
the selected script before running a credentialed or external-service group.

`just e2e-group 17_multi_node` starts a real Cloud Auth fixture, one Talos Server,
and three Talos Workers in isolated homes. Set `TALOS_CLOUD_SERVER_BIN` to the
fixture executable or make `cloud-server` available on `PATH`; no Cloud source
checkout is required. It also needs `protoc`, Python `grpcio`, `protobuf` and
`PyYAML`. Execution scenarios also require a real provider key in `.env`. The attachment
scenario uses `deepseek-v4-flash-vision-exp` by default; `TALOS_E2E_IMAGE_MODEL`
can select another configured, accessible vision model.
Management uses the existing protobuf descriptors and public RPCs; SQLite is
opened read-only for persistence assertions. No fake Provider or direct database
registration is used. Set `TALOS_MULTI_NODE_KEEP_TMP=1` to retain fixture data
for diagnosis; retained homes contain test credentials. A failed acceptance
scenario is reported as FAIL even when it exposes a known implementation bug.

Computer registration and outbound execution use `18_computer_workspace`.
Registration alone: `just e2e-case 18_computer_workspace/01_registration`, with
Caddy (`TALOS_E2E_CADDY_BIN`), Node.js and pnpm dependencies; no model key required.
The full group also requires funded execution/inner/vision providers. Real
execution acceptance has resumed after provider funding. See the
[scenario contract](scenarios/18_computer_workspace/README.md) for scope and evidence.

Multi-control-plane synchronization uses `just e2e-group 19_computer_sync` on a
Mac with Docker, the pinned public Sync v0.1.3 image, a Cloud fixture executable,
and the same protobuf/Python/Node prerequisites. The group builds the current Worker
image and pulls the immutable Sync Release digest without a sibling source checkout.
It starts isolated real services and checks owner isolation, hardware
registration, realtime history, same-Computer continuation, offline outbox,
and restart recovery. Only execution/recovery cases need funded model access;
registration does not. See the [scenario contract](scenarios/19_computer_sync/README.md).
The Cloud frontend is not required: the fixture uses real device authorization APIs.
This complete macOS/Docker/Cloud topology remains local acceptance, separate from
the secretless hosted PR gate. Its isolated Docker-context regressions run in CI.

## Evidence and temporary data

Generated evidence is written under `talos-e2e/reports/`. Temporary Talos homes
and scenario data use `talos-e2e/.tmp/` unless `TALOS_E2E_TMP_ROOT` overrides
the location. These directories are generated output, not source documentation.

All discovered cases use the same before/after cleanup boundary. Each case gets
private temp/home/native-output paths, an owned shell process group and a private
tmux server. Resource-owning fixtures register Docker resources and detached
processes through the [lifecycle contract](LIFECYCLE.md). Cleanup runs after
success, failure and INT/TERM; cleanup errors fail the run and retain ownership
records for diagnosis. Reports survive cleanup. Only this invocation's resources
are removed; shared images, supplied binaries and package/build caches stay intact.

Images built by the runner use a unique run tag/label, are reused between cases,
and are removed at run end. Image overrides are pre-existing inputs and are never
rebuilt or deleted. Avoid global `docker prune` operations.

Binary overrides (`TALOS_E2E_SERVER_BIN`, `TALOS_E2E_CLI_BIN`,
`TALOS_E2E_WORKER_BIN`, `TALOS_E2E_APP_SERVER_BIN`,
`TALOS_E2E_APP_SERVER_TEST_CLIENT_BIN`, `TALOS_E2E_SANDBOX_BIN`) must point to
executables. The runner resolves and exports them once; an invalid explicit path
fails without fallback. Otherwise it uses the existing locked Cargo build.
On Linux, reuse the sandbox launcher beside the chosen binary, or build only the
matching debug/release launcher when it is absent.

## Test rules

- Validate the real user path and observable effect, not only an `OK` response.
- Keep deterministic tests independent of unstable time and external networks.
- Use real services only in scenarios that explicitly own that integration.
- Reuse shared runner helpers and fixtures instead of duplicating setup.
- Preserve logs or reports that prove the critical behavior when manual evidence is required.

`06_tools/cli_bash` accepts an upstream response with no reasoning text, including
when thinking is enabled. It still requires successful Bash execution and matching
tool-call identities; it does not require a synthetic thinking event. Deterministic
Provider transport regressions verify empty completion and exact nonempty reasoning
and argument preservation. `13_interaction/user_action_http` retains exact command,
approval, side-effect, and terminal-history checks; entity-escaped commands do not
qualify as equivalent commands.

Group `12_files` retains Local attachment and S3 upload/list/read/delete coverage.
Its S3 helper owns a standalone Versity Gateway process and temporary directory
under `scenarios/12_files/.tmp/`; the scenario EXIT trap stops the process and
removes the directory, including on failure. No Docker or MinIO client is needed.
