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
| `09`-`10` | App Server protocol, Skills, MCP, Web, Desktop, and SDK regression |
| `11`-`13` | Cloud auth, files, and durable user interaction |
| `14` | Unified sandbox and permission enforcement |
| `15` | Release bundle installation and installed TUI startup |
| `17` | Three Workers: public registration, owner isolation, routing, lease renewal, collaboration and Workspace ownership |
| `18` | Computer workspace: real Desktop device login, durable UUID, outbound registration and renewal |
| `19` | Multi-control-plane sync: two native Mac Desktop runtimes, native and Docker Workers, Docker WSS hub, real auth and model tasks |

Inspect `scenarios/` for the current case list; it is the source of truth.
There is no current group `04`; use the scenario directory rather than assuming
that group numbers are contiguous.

The approval-gated GitHub PR quality gate runs the secretless scenarios
`01_health_and_models`, `09_app_server/smoke_e2e`, `10_talos_app`, `12_files`, and
`14_sandbox`, along with the `talos-auth` and `talos-base` package tests for the
public Cloud SDK adapter and Desktop enrollment boundaries. `15_tui_install` has
its own four-platform native and Linux Docker gate. The PR workflow does not
receive repository secrets and does not check out private repositories.

Maintainers can manually run the provider-backed `trusted-e2e.yml` workflow from
`main`, including the full `09_app_server` group whose execution, MCP, memory and
skills cases need real model credentials. The public app-server smoke case reuses
the existing client to exercise model listing, Session creation, configuration and
Workspace/Memory API reads without executing a model. Scenarios `11_auth`,
`17_multi_node`, and `18_computer_workspace` still
use the configured Cloud fixture executable for isolated account setup.
The larger `19_computer_sync` topology also remains local acceptance. These
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
