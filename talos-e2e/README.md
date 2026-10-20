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
just e2e-case 01_health_and_models
just e2e-group 09_app_server
just e2e-group 14_sandbox
```

Direct runner usage is also supported:

```bash
talos-e2e/run_e2e.sh --case 09_app_server/protocol_e2e
talos-e2e/run_e2e.sh --group 15_tui_install
```

The runner prebuilds only binaries required by the selected scenarios. Prefer a
single case during development and the owning group before handoff.

## Scenario ownership

| Groups | Scope |
| --- | --- |
| `01`-`03` | Server health, models, Session lifecycle, HTTP, and SSE |
| `05`-`08` | CLI, tools, memory, and embedded TUI |
| `09` | App Server protocol, Skills, MCP, and Memory integration |
| `11`-`13` | Cloud auth, files, and durable user interaction |
| `14` | Unified sandbox and permission enforcement |
| `15` | Release bundle installation and installed TUI startup |
| `17` | Three Workers: public registration, owner isolation, routing, lease renewal, collaboration and Workspace ownership |
| `18` | Computer workspace: real Desktop device login, durable UUID, outbound registration and renewal |
| `19` | Multi-control-plane sync: two native Mac Desktop runtimes, native and Docker Workers, Docker WSS hub, real auth and model tasks |
| `20` | Browser MCP and resource workspace: shared Web/Desktop tabs and local Markdown, native browser host/MCP, Docker desktop/media/control and profile recovery |

Inspect `scenarios/` for the current case list; it is the source of truth.
Groups `04` and `10` are not assigned; use the scenario directory rather than assuming
that group numbers are contiguous. Frontend unit tests belong to the frontend
quality workflow and are not repeated as an E2E scenario.

The approval-gated GitHub PR quality gate runs the secretless scenarios
`01_health_and_models`, `09_app_server/smoke_e2e`, `09_app_server/desktop_recovery`, `09_app_server/computer_deletion`, `20_browser_workspace/01_resource_tabs`, `12_files`, and
`14_sandbox`, along with `talos-auth`, `talos-base`, `talos-storage`,
`talos-agent` and `talos-service` package tests. These cover public Cloud SDK
adapters, Desktop enrollment, actual SQLite cancellation/connection reuse,
Worker reservation barriers, and the imported-history/Shared-confirmation race. `15_tui_install` has
its own four-platform native and Linux Docker gate. The PR workflow does not
receive repository secrets and does not check out private repositories.

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
English/Chinese prompts, clearing selection and rejected old credentials. Run
`talos-e2e/scripts/computer_workspace/deletion.py` without `--contract-auth` for
local acceptance with real disposable Cloud Auth (`TALOS_CLOUD_SERVER_BIN`). Both
modes require Python Playwright and installed Chrome (`TALOS_E2E_CHROME_BIN`).
This case does not exercise a multi-manager Sync deployment; that topology
remains local group 19 acceptance, with deletion/history covered separately by
owning-package two-peer Store/CRDT tests.

`20_browser_workspace/01_resource_tabs` mounts the actual product layout in an isolated
system Chrome instance against real Server/File APIs. Only upstream Auth is a
contract fixture. Deterministic local previews cover independent same-URL tabs,
retained form state across switching/collapse, keyboard selection, native modal
and Escape focus, close/account-scope destruction, and authenticated Markdown
rendering with non-owner metadata/content rejection. Web-only changes run this
case alongside Computer registration without requiring a Docker Worker or model.
Browser owner changes run the complete numbered [group 20](scenarios/20_browser_workspace/README.md)
once through the standard runner in the dedicated Browser job; the generic matrix
does not duplicate its resource case.
Computer live media teardown is covered separately by the Browser Worker
acceptance and the owning ComputerDesktop component tests; this lightweight case
does not claim a live remote desktop connection.

`09_app_server/desktop_recovery` uses the real controller, SDK, Server and
app-server processes with controlled loopback Auth/model HTTP fixtures. It checks
actual credential enforcement, SIGTERM replacement, exact delivered/history event
parity, unchanged Computer/Session, a subsequent controlled turn and the original
unsubscribe handle. It needs no private source or provider credentials. Native
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
Mac with Docker, the adjacent `talos-sync` source, a Cloud fixture executable,
and the same protobuf/Python/Node prerequisites. The group builds current Worker and Sync
images, starts isolated real services, and checks owner isolation, hardware
registration, realtime history, same-Computer continuation, offline outbox,
and restart recovery. Only execution/recovery cases need funded model access;
registration does not. See the [scenario contract](scenarios/19_computer_sync/README.md).
The Cloud frontend is not required: the fixture uses real device authorization APIs.

## Evidence and temporary data

Generated evidence is written under `talos-e2e/reports/`. Temporary Talos homes
and scenario data use `talos-e2e/.tmp/` unless `TALOS_E2E_TMP_ROOT` overrides
the location. These directories are generated output, not source documentation.

## Test rules

- Validate the real user path and observable effect, not only an `OK` response.
- Keep deterministic tests independent of unstable time and external networks.
- Use real services only in scenarios that explicitly own that integration.
- Reuse shared runner helpers and fixtures instead of duplicating setup.
- Preserve logs or reports that prove the critical behavior when manual evidence is required.

Group `12_files` retains Local attachment and S3 upload/list/read/delete coverage.
Its S3 helper owns a standalone Versity Gateway process and temporary directory
under `scenarios/12_files/.tmp/`; the scenario EXIT trap stops the process and
removes the directory, including on failure. No Docker or MinIO client is needed.
