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

The GitHub PR quality gate runs all existing E2E cases and groups except
`19_computer_sync`. That group needs a Mac host plus multiple Docker services and
is a local topology acceptance. It must be run and reported locally; its exclusion
does not waive gated coverage for agent or product behavior.
Group `15_tui_install` is covered by the separate TUI install gate, including four
native platform runners and the Linux Docker install case.

## Environment requirements

| Scope | Additional requirement |
| --- | --- |
| Base server, CLI, and TUI | Rust toolchain and repository test tools |
| Web and Desktop | Node.js, pnpm dependencies, and platform UI prerequisites |
| Real model or MCP flows | Network access and the provider credentials required by the scenario preflight |
| Cloud auth and S3 | Explicit cloud test credentials and isolated resources |
| Linux sandbox supplements | Docker on a supported host |
| Release installation | Native target environment; Docker supplements Linux only |

Each scenario owns its preflight and reports missing variables or tools. Read
the selected script before running a credentialed or external-service group.

`just e2e-group 17_multi_node` starts a real Cloud Auth server, one Talos Server,
and three Talos Workers in isolated homes. It requires the adjacent `talos-cloud`
checkout (or `TALOS_CLOUD_REPO`), `protoc`, and Python `grpcio`, `protobuf` and
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
Mac with Docker, the adjacent `talos-sync` and `talos-cloud` sources, and the same
protobuf/Python/Node prerequisites. The group builds current Worker and Sync
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
