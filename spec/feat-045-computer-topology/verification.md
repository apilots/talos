# Computer topology verification

Date: 2026-10-14. Scope: isolated worktree `design/computers-topology`, based on fetched `origin/main` at `696e4699`. Sync phase 1 local readiness completed before this implementation. No commit, push, publication or production deployment occurred.

## Observed behavior

The real Computer management route preserves the 56 px rail and fills the right-hand area. Entering an individual Computer preserves the existing 296 px resource sidebar (workspace begins at x=352). Inspection is independent of execution selection. Topology and list retain the same inspected Computer, server search and explicit pagination. Counts distinguish account totals from availability in the loaded directory window.

Only authenticated current-account Sync observations create synchronization paths. The original AuthService verifies the caller, the configured HTTPS Sync origin verifies its Bearer token again, and the adapter rejects wrong-account replies. Account-B browser acceptance showed no account-A Computer names or inspector residue. Account metrics do not implicitly enable synchronization or register a Worker. The public monitor never supplies private topology.

Fixed Control Plane and Sync nodes, bidirectional execution paths, presence-backed Sync paths, absent observations and offline Computers were checked through real native Talos/Sync/Worker processes. A pure Worker received no direct Sync edge. Disabling local management sync removed its Control Plane observation while independent Worker execution remained available. Restoring sync recovered that relationship. A deliberate Metrics-only proxy outage retained execution state, removed private Sync observations and showed an explicit unknown/error state; automatic polling recovered after the outage ended.

Chinese and English were switched through the existing Talos settings. Both locales render the topology/list and detail labels; dates and numbers follow locale. At 390 px, document width remains 390 and the graph/table scroll internally with a visible hint. Existing rename and disable/enable were exercised on an owned fixture and restored. The browser fixture uses synthetic identities and a loopback SSO proxy; actual AuthService, HTTPS trust and upstream Sync authorization remain enabled. No browser state injection or fabricated metrics response was used.

## Checks and results

| Check | Observed result |
| --- | --- |
| Canonical `scripts/sync-metrics-contract.py --check` | Passed; committed DTO/schema digests and generated account subset match |
| `just test -p talos-sync` | 19/19 passed, including bounded HTTPS/account/version adapter and ephemeral own-alias publication coverage |
| Protocol and Rust SDK scoped nextest | 28/28 passed |
| talos-base, Rust SDK and embedded SDK compile / strict Clippy | Passed |
| App Server and TUI compile | Passed |
| E2E-feature scoped Clippy | Passed; existing MCP ClientInfo deprecation warnings remain |
| Real native cross-project acceptance | Passed in 26.176 seconds; presence, original auth, actor/token mismatch, second-account isolation, HTTP/RPC projection and original same-Worker continuation |
| `pnpm --dir talos-app ci:frontend-app` | Passed: lint, typecheck, build, repository/error guardrails, contract check and all enabled frontend suites |
| Final frontend tests | Core 58, Browser 10, App Server 28, Desktop 37 and Web 399 passed (532 total); two existing optional integrations were not enabled |
| Final Web lint/build | Passed after final mobile hint and failure-context changes |
| Rust formatting and change-scope regressions | Passed; 7/7 scope cases |
| Browser acceptance | Actual desktop/mobile, zh/en, topology/list, independent inspection, original shell, account switch, failure/recovery and fixture management passed |

The two unrun tests are the existing SDK stdio integration and Desktop app-server crash-recovery integration, each gated by its original environment flag. They are not counted as passes. Desktop IPC guards, shared UI and typed SDK parity have enabled regression coverage; an actual Electron UI session is not claimed.

Reproduce the resource-intensive local topology acceptance with a locally built Sync binary:

```sh
TALOS_SYNC_BIN=/absolute/path/to/talos-sync/target/debug/talos-sync just test -p talos-base --features e2e shared_session_continues_on_the_same_worker_and_active_commands_return_to_its_coordinator
```

Representative TLS/auth/wire, presence and client parity cases are included in the hosted PR workflows. The hosted workflows have been updated, but no hosted execution is claimed here. Full native multi-host topology acceptance remains explicitly local; see [E2E scope](../../talos-e2e/README.md).

## Contract and delivery

The committed [contract manifest](../../contracts/talos-sync/metrics-v1/manifest.json) records the canonical Sync DTO/schema identity. Refresh with `python3 scripts/sync-metrics-contract.py --source /absolute/path/to/talos-sync`; builds need no adjacent checkout. Regenerate public app protocol artifacts with the existing `just gen-app-protocol` owner when product DTOs change.

Transient logs, screenshots and fixture scripts are in ignored `output/` and `.tmp/`. The original Talos checkout still has only its four pre-existing configuration modifications. This result establishes local implementation acceptance; source review, formal hosted gates and any release/publication remain separate.

## Pre-commit review, 2026-10-15

Reviewed all task changes across the canonical contract, verified credential owner, HTTPS adapter, HTTP/RPC/SDK projection, presence lifecycle and frontend account/query/inspection scopes. Every account read re-verifies the original caller, sends its Bearer token only to the configured HTTPS origin without redirects, bounds time/body size and rejects a mismatched returned account. Public aggregate monitoring is not used for private topology. The Sync-side 24-hour bound remains the authoritative retention owner; Talos does not create a parallel metrics store.

Fixed a missing explicit `Cache-Control: no-store` on the private HTTP Metrics response, including handler errors. The real HTTP acceptance now asserts this header. Added a frontend regression proving that a late previous-account response cannot replace the current account's observations. Fixed singular English unmapped-peer wording and added locale-key and interpolation/plural regressions for both languages. Replaced an explicit mutex-guard drop in a test with a lexical scope to satisfy test-target Clippy; no production locking behavior changed. Checking the TUI test target also found an incorrectly added SDK method in the TUI-owned mock trait implementation; removed that unrelated method, restoring the mock file to its original content. Safe backend English error reasons remain visible under the existing nullable typed-localization contract, with localized page context; no string-based error classification was introduced.

Fresh results: full `pnpm --dir talos-app ci:frontend-app` passed, including lint/typecheck/build, repository/error guards, canonical contract check and 536 enabled tests (Core 58, Browser 10, App Server 28, Desktop 37, Web 403). The same two pre-existing optional stdio/Desktop process integrations remain disabled, not passed. Scoped Rust nextest passed 47/47; `just fix` was run on the affected Rust owners and followed by strict test-target Clippy with no new warnings. Real cross-project presence/auth/HTTP/RPC/isolation and same-Worker continuation acceptance passed again in 26.267 seconds, including the no-store assertion. The complete TUI suite passed 223/223 after removing the invalid mock method. Commit-subject and seven change-scope regressions passed. Canonical source/snapshot comparison still passes against local Sync commit `439897fe481cf9f1c21a7b8f7d39cba3e14a0c20`; the wire contract did not change during review.

The user authorized local project commits after this review. No push, source tag, release or production deployment is authorized. Hosted workflow execution and an actual Electron UI session remain outside this local result.

## Bot review follow-up, 2026-10-15

- Accepted [P1 optional presence failure](https://github.com/apilots/talos/pull/28#discussion_r4278218152). An unsupported presence join reproduced a document-sync timeout before the repair. The runner now reports publication errors without terminating core document transport. Controlled real-WSS regressions verify both join and update rejection, retained connection/outbox semantics and subsequent document commits; all three controlled runner scenarios pass.
- Accepted the missing-mapping issue in [P2 delayed enrollment](https://github.com/apilots/talos/pull/28#discussion_r4278218160). Directly rewriting saved aliases would violate immutable Store capture associations and binding validation. The original verified host owner instead refreshes the existing handle's ephemeral local Computer observations through Tokio watch, after ownership verification. This creates no additional task/registry, durable binding rewrite or execution grant. Real native acceptance starts a valid account connection with an empty saved binding, later enrolls the Worker through the original status flow and verifies its presence appears on the same handle while the saved binding stays unchanged.

The expanded real cross-project scenario passed in 36.485 seconds, including original HTTP/RPC/auth/account-isolation/no-store and same-Worker continuation. Full focused Rust nextest passed 49/49, strict test-target Clippy (including e2e), rustfmt and canonical contract verification passed. The original three control scenarios and new real enrollment case remain covered; push hooks run again before delivery. Canonical Metrics v1 source/snapshot comparison remains unchanged. No merge or release.
