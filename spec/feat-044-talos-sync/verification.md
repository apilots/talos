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
