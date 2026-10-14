# Talos multi-host synchronization acceptance

Status: Complete. Date: 2026-10-13. This record keeps acceptance facts;
transient logs, screenshots, candidate JSON and intermediate review reports are
excluded from source control and preserved locally under
`output/validation/feat-044-talos-sync/20261013-precommit-cleanup/`.

## Product behavior and ownership

- G owns Worker registration, leases, Connect/NAT routing and callbacks to the
  current coordinator. H only synchronizes account data over WSS/TLS + Loro.
- Original management Stores capture/materialize facts. A Session remains bound
  to one Computer/Worker; idle continuation can use another authorized management
  host. Active execution and workspace files remain on that Worker.
- Sync choice belongs to the current host/account in the existing management DB,
  including before a first hub binding. Deployment `sync.enabled=false` disables
  H participation for every account on that host without changing G/RPC execution.
- Public dependencies are `talos-sync-sdk = "=0.1.0"`,
  `talos-sync-types = "=0.1.0"` and `talos-cloud-sdk = "=0.1.3"`.
  Production builds and CI do not require private SDK source checkouts.

## Observed verification

| Scope | Result |
| --- | --- |
| Frozen product candidate, `just e2e-group 19_computer_sync` | 8/8 PASS; 868.81 seconds; run `20261013_120647-27103` |
| Public Sync registry integration, Store/Service/Sync/Base | 369/369 nextest PASS; 134.209 seconds; 0 skipped |
| Commit-time app-server/Session Store module split, App-server/Store/Base/Sync | 187/187 nextest PASS; 100.420 seconds; 0 skipped |
| Public Cloud SDK authentication adapter | 17/17 nextest PASS; 6.859 seconds; 0 skipped |
| Rust SDK and embedded SDK affected event-stream adapter | 14/14 nextest PASS; 0 skipped |
| Real device authorization and registration, existing 18/01 | PASS; ten checks; 81.31 seconds; run `20261013_134101` |
| Original changed Rust packages | 22-package `just fix` PASS; final auth/Store/app-server strict Clippy PASS |
| Independent Sync service after public Cloud SDK migration | 20/20 PASS; real Docker WSS SQLite and PostgreSQL acceptance plus two PG durability scenarios PASS; owned by the Sync project |

The eight-case group covers registration/hardware and live status, full history
and same-Computer continuation, recovery/account isolation, offline local work,
resource interactions, independent cloud faults, real NAT routing, and owner
model/search configuration. It uses actual Desktop management runtimes, a Mac
Worker, Docker Workers and isolated management containers. No SQL-injected
business facts or mock model execution replace these behaviors.

The 19/03 recovery case includes failed first H discovery, disabling without a
binding, restart persistence, H recovery, explicit re-enable and deployment
policy over multiple local accounts. Worker input, child-agent waits, source
FileService, final history cursor commits and current-coordinator resource
callbacks are included in the group.

Web and native Desktop were observed with the same frozen candidate after
registration recovered, including all four Computers, real hardware, the local
Computer label and owner settings. Those screenshots remain local validation
artifacts rather than specification source.

## Pre-merge regression closure (2026-10-13)

Unconsumed Worker admissions now restore the entire previous marker; an abandoned
first admission removes its marker durably. Consumed executions retain the
original cleanup and history-commit requirements. Direct clients finish the
TLS/HTTP2 connection before publication, and deployment guides use the dedicated
Worker image. Shared theme buttons, MCP actions and unchanged-save states are
included in the same correction.

- Agent, Client and Base: **96/96 nextest PASS**, 70.012 seconds, zero skipped;
  scoped `just fix`, strict test-target Clippy and formatting checks passed.
- Updated **19/05 resource interactions PASS**, with eight observable checks.
  The new cases reject unreachable/TLS-invalid Direct registration and disconnect
  B after actual paused admission. With B and H stopped, A resumes the same
  Session; no abandoned Turn history or tool side effect appears. Original source
  files, live input/queue/cancel, child callbacks, clear/delete and offline waiting
  configuration checks also passed.
- The Save regression confirms login-managed credentials remain read-only and
  unchanged submission performs no save or model refetch. Light/dark Server and
  MCP visual checks are preserved in ignored root output.

This is an affected-scenario regression, not a new complete eight-case group run.
All new logs, candidate files, service homes and screenshots stay in ignored
root `output/`; no production deployment or standalone Sync code change is included.

## Verification limits

The complete eight-case run preceded the public Cloud SDK packaging migration
and the mechanical module split. Their affected paths were verified separately
as recorded above; the complete group was not repeated. See
[Cloud SDK integration](verification-cloud-sdk.md) for its exact scope.

This acceptance used one arm64 Mac, separate management processes and real Linux
Docker networks. It does not claim multiple physical machines, Kubernetes,
production PG or a production deployment. Public SDK publication does not imply
a service release. The independent Sync project owns its service checks; both
projects build against the public Cloud SDK without private SDK credentials.

## Post-merge maintenance (2026-10-14)

The 043 records now explicitly identify the superseded cloud Desktop assembly;
the current UI plan uses each management host's original DB and Worker identity.
`talos-tools-sandbox` declares its own Tokio `fs`, `io-util` and `process` features,
without relying on a consumer to enable them. Standalone package check, 41/41
nextest tests (zero skipped), all-target strict Clippy, formatting, repository
checks and all 24 local links in the edited 043 documents passed. Installer and
release contract checks passed, including the historical release archive fallback.
These are scoped maintenance checks, not a repeated eight-case sync acceptance
or a production deployment.

The scoped native installer correction subsequently passed hosted acceptance:
run `38499832576` completed all four Linux/macOS amd64/arm64 installations,
Linux Docker installation and the required aggregate gate. This proves the
45-minute job budget permits the existing tests and cleanup to finish; it does
not claim a tagged product release. The closure PR is rebased onto the public CI
policy, with its temporary external-PR workaround removed.
