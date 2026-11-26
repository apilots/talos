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
| `09` | App Server protocol, Plugins, Skills, MCP, and Memory integration |
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

`05_cli/exec_unattended` uses a loopback model to check waiting and failure
exit codes, Session permission persistence, and token usage on stderr. It also
checks that a spawned subagent inherits the parent Session permission and can
run a tool requiring approval under the default policy. The scenario installs
an unrelated Plugin during a live turn, verifies that competing execution is
refused, and recovers the Session after SIGKILL. Run it with
`just e2e-case 05_cli/exec_unattended`. The native ARM64 Harbor smoke trial
is documented in [Talos Agent benchmarks](../talos-bench/README.md).

Plugin lifecycle has two secretless entry-point cases. Run
`just e2e-case 09_app_server/plugin_api_lifecycle` for the Desktop UI's App Server
API. It covers catalog search, document reads, and Skill and MCP management.
Run `just e2e-case 05_cli/plugin_lifecycle` for the CLI's Marketplace
commands, legacy Browser migration at startup, installations by name, and offline package replacement. Plugin
backend and shared catalog fixture changes select both cases.

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

### Session automation group

`just e2e-group 24_automation` owns Goal/Loop product acceptance. The existing
runner discovers its three cases from the filesystem and their declared prerequisites:

- `01_session_automation`: shared lifecycle, cadence, mutual exclusion, Queue/Steer,
  restart and CLI/SDK/Worker contracts described below.
- `02_web_entry`: actual Add and slash selections, original typed input, first-work
  countdown/cancel, automatic Timeline output and terminal row removal. It also
  verifies the Composer policy's Worker YAML persistence, reload and same-Turn
  model context using an isolated Server, production Web assets and Chrome.
- `03_tui_entry`: actual `/goal <prompt>` and `/loop <prompt>` through the original
  terminal/SkillSelection path, automatic stream display and terminal task cleanup.

All three reuse the local controlled model fixture; no provider key is required.
Shared fixture changes select all three cases. Web and TUI entry changes select their
own carriers. The lifecycle case is no longer bundled into the app-server smoke target,
so it runs once when ownership selections are combined. Client cases supplement,
rather than repeat, the full lifecycle matrix.

`just e2e-case 24_automation/01_session_automation` uses isolated homes and controlled
loopback model/Auth upstreams with real System Skills, Automation Tool, Service,
queue, settlement and History. It runs the shared business assertions through
app-server and the embedded SDK, including a real one-minute fixed Loop interval
in each transport, ordinary User Queue/Steer inside automatic Turns, dynamic
decision validation, revision changes and Stop. The same real Tool path also
checks model-decided Stop at normal settlement, a fixed next time beyond seven
days (10081 minutes, no expiry field), and a Goal continuing through two Turns
with unchanged creation time and a frozen completion update timestamp. A malformed
nested model-decided schedule is rejected through the real Tool; the model
then lists an empty state, corrects Create in the same Turn and finishes with
a normal model Stop. Fixed and dynamic fixture inputs state their requested
cadence explicitly; controlled responses do not prove general language inference. It also hard-restarts only its
own app-server to distinguish durable definitions from the original memory queue.
The script explicitly runs the ignored `automation_worker_business` tests with
`just test -p talos-base --run-ignored only -E 'test(automation_worker_business)'`
for real `direct` gRPC Connect and `outbound` WSS registered Workers, including in-flight WSS
interruption with the original typed cancellation settlement. Default ignored status is not
acceptance evidence. No provider credentials, private source or business API mocks
are used; unrelated MCP tools are disabled. Browser input/rendering remains a
separate Web/Desktop acceptance responsibility.

The same Automation case also runs the actual `talos` CLI against the shared controlled model: Goal start/complete and pause/resume retain the foreground host; Loop survives first settlement and exits after a separate exact-ID Stop or its actual model-decided Stop. This short process check adds no second minute-cadence matrix.

`09_app_server/desktop_recovery` uses the real controller, SDK, Server and
app-server processes with controlled loopback Auth/model HTTP fixtures. It checks
actual credential enforcement, SIGTERM replacement, exact delivered/history event
parity, unchanged Computer/Session, a subsequent controlled turn and the original
unsubscribe handle. It also verifies Cloud-offline local metadata, complete History,
status, Live subscriptions and idle deletion without additional Cloud requests.
Set `TALOS_E2E_SYNC_HUB_BIN` to the independent Hub executable to exercise real
WSS isolation with the same controlled local account and model. The scenario
stops the Hub, then holds the restarted Hub's SQLite writer with 379 batches in
the Desktop outbox. Both cases verify local completion, cancellation through
`turn/interrupt`, closure of the model stream, and a successful following Turn.
Add `TALOS_E2E_SYNC_STRESS=1` for local acceptance with 12,128 synthetic Timeline
facts. After the Hub starts acknowledging the original batches, the same
foreground checks run while some of those batches remain. The backlog must
clear within 120 seconds of unlocking the Hub.
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

For generic-protocol recovery, build the independent Sync repository's current
compatible Hub executable, then run from the Talos root:

```sh
TALOS_SYNC_BIN=/absolute/path/to/talos-sync \
  just test -p talos-base --features e2e \
  -E 'test(application_sync_tests::) or test(directory_reads_remain_local)'
```

This local acceptance starts an actual Hub child process, a TLS Talos Server
and native management/Worker runtimes with separate homes and databases. Auth
and model responses are controlled local fixtures; it needs no production
credentials or paid-model requests. It covers account boundaries, automatic
enrollment, settings/restart, remote Worker command forwarding, complete History,
live updates, reset/deletion, offline local reads and pending writes. Its recovery
extension stops source A and the old Hub, forces B's first migration to fail,
checks automatic retry into an empty replacement Hub, restores cold C's complete
history with original identities/timestamps, while the Computer directory comes independently from Server, then verifies
the following incremental notice and local reads after the replacement Hub stops.
The recovery fixture includes retained schema-v1 snapshots generated by the published SDK 0.1.7, not a hand-built v1 JSON substitute. It verifies all four native Turns and the following delta. The directory regressions block real TLS Server responses and check local reads, whole-page publication, stale status, account isolation and mutation write-through with Sync disabled. The connection/directory subset also
holds real TLS rename responses across confirmed deletion, complete-directory removal
and a newer rename; reverse-order fixtures commit create/rename after an older
snapshot and verify that the create credential survives. Cache restart acceptance
stops both local Server and Auth endpoints, then reads the retained owner directory
and details through the native service using the already established account identity.
It does not claim a fresh offline login or packaged Desktop cold-start acceptance.
Automatic enrollment acceptance also starts with an empty local capture association,
creates a Project before registration, then verifies its baseline and following
rename are acknowledged by the real Hub. The focused local case is
`automatic_enrollment_recovers_project_baseline_and_following_increment` with
`--features e2e` and `TALOS_SYNC_BIN` configured. The hosted subset covers the
same local baseline/increment capture for an existing handle and a retained binding
without any Hub, preserves disabled Sync, and checks first discovery and saved
transport can dial while the Worker Gateway is stalled.

Run this hosted-compatible subset without a Hub:

```sh
just test -p talos-base -E 'test(connection_tests)'
```

These native checks do not claim packaged Desktop,
Docker Worker or two-physical-machine acceptance. The separately pinned Group 19
image must support the candidate SDK protocol before running that larger group;
its older public Hub image is not evidence for generic-protocol recovery.

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
Mac with Docker, the pinned public Sync v0.1.6 image, a Cloud fixture executable,
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

The runner writes a separate `reports/e2e_results_<initial-run-id>.md` with
numbered group summaries and case tables. Each execution has its own duration,
estimated cost, outcome, revision and log under `reports/runs/<run-id>/cases/`.
Execution duration excludes runner evidence collection and cleanup. Case totals
include all executions. The original report and the traditional
per-case paths remain available. Use the initial run ID printed by the runner to
add only the affected case to the same report:

```sh
talos-e2e/run_e2e.sh --case 05_cli/full_flow --rerun-of <initial-run-id> \
  --note 'The connection remained open after cancellation → close it on exit (fix abc123)'
```

Provide one short note per repaired case with the cause and fix. The table
supplies the execution number, rerun count and observed outcome. The first
result and its evidence are kept. Rates use PASS and FAIL for the denominator;
blocked, cancelled and unexecuted cases appear separately from these counts.

Usage is read before runner cleanup from existing Profile snapshots in
case-owned Talos homes. The model column shows the configured provider/model,
and duplicate snapshots are counted once. A snapshot can lack failed,
cancelled, remote or not-yet-persisted usage. The report identifies partial
coverage and shows N/A when it cannot estimate cost. These reads make no model
requests.

Pass `--prices /path/to/prices.json` or set `TALOS_E2E_PRICES` for the initial
run to supply explicit rates per million tokens.
The runner saves the rates, currency, source and date with each run. A targeted
rerun inherits the initial snapshot unless overridden. Use an explicit
`free: true` with four zero rates for a free fixture. If a cache count is
missing, its fee can be estimated only when that rate equals the ordinary input
rate. Currencies are reported individually. Example rates for a local fixture:

```json
{
  "fixture/model": {
    "currency": "CNY",
    "source": "Local fixture example",
    "as_of": "2026-01-01",
    "input": 2,
    "output": 5,
    "cache_read": 0.5,
    "cache_write": 2
  }
}
```

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

`06_tools/public_deepseek_recovery` uses the dev YAML and the real public
`https://api.deepseek.com` `deepseek-v4-flash` model. It requires
`TALOS_LLM_DEEPSEEK_API_KEY`, performs a successful Bash call, a deliberate Read
failure, and a recovery Bash write, then continues the same Session with another
tool call. It checks streamed events, canonical call/result identities, terminal
history, model-request tracing and exact file effects. Run it locally with
`just e2e-case 06_tools/public_deepseek_recovery` or through the existing
maintainer-only `06_tools` workflow; public PR gates do not receive model secrets.

### Long running Turn JSON load

`just e2e-case 09_app_server/running_turn` uses the production Web build and an
isolated system Chrome with explicit JSON Session/SSE fixtures. Computer and
metadata APIs use a real Server. It covers a single three-hour-shaped Turn at a
large offset with 8, 500, or 3000 tool calls, refresh, earlier-step access, a live
1000-delta burst and Sidebar navigation. It verifies that a folded Turn hides
its process, that the expanded rail keeps commentary and Thinking in sequence
with the tools, and that a long interleaved history renders bounded pages. It
also checks the quiet-command timer and the first streamed output after
refresh and navigation. The case saves the input JSON, screenshots and timings.
The represented duration is fixture data; this test does not wait three hours or
claim model/runtime endurance. Service/HTTP tests separately verify that a real
canonical Turn read and the offset subscription agree. The WebUI hosted gate
selects this case for chat, rendering and Timeline ownership changes.


### Existing-Hub and local startup migration

A separate local-only acceptance consumes a prepared released-v1 Hub database
and the current independent Sync executable. The case is explicitly ignored in
the hosted set because it requires these cross-project artifacts; invoking it
without either input fails rather than reporting a simulated pass.

The Sync checkout provides the reproducible old-database producer. Run there:

```sh
python3 e2e/scripts/prepare_v1_database.py .tmp/source-a-v1.db
just build -p talos-sync-server
```

Then run from the Talos root with absolute paths to those outputs:

```sh
TALOS_SYNC_V1_DATABASE=/absolute/path/to/sync/.tmp/source-a-v1.db \
TALOS_SYNC_BIN=/absolute/path/to/sync/target/debug/talos-sync \
  just test -p talos-base --features e2e --run-ignored only \
  -E 'test(upgraded_hub_and_local_replica_continue_from_the_original_cut)' \
  --success-output final
```

The fixture database contains SDK0.1.7 source A's two complete Turns, its Hub
identity and original SQLx migration ledger. The test copies it to an isolated
home, starts the upgraded Hub with the Parser-owned mapping profile, and lets
cold native C restore those two Turns while B is absent. It then starts a local
B installation containing the original four-Turn v1 checkpoint with only A's
cut acknowledged. Talos startup upgrades B before authentication; its causal
suffix extends C to four complete Turns and the next notice. The independent
Hub service owns the prepared database generator and its migration checks;
Talos does not replicate the Hub's private DDL.

The existing new-Hub disaster-recovery case remains separate. Its released-v1
local checkpoint now goes through a real native runtime restart with a released
installation's task ledger, not conversion inside `bind_account`.
