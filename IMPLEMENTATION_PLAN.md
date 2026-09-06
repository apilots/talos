# Unified Error Contract: Review Handoff

PR #16 review follow-up is implemented and verified. The error taxonomy is frozen; reuse existing state,
Timeline, and Store boundaries without adding a cache or framework.

## Stage 1: Initial-input ownership
**Goal**: Clear settled requests' pending state across session navigation.
**Success criteria**: Returning to the original session enables its query after
either success or failure; another pending owner is never cleared accidentally.
**Tests**: Extend the existing route-navigation regression, then lint/typecheck.
**Status**: Completed. Both navigation outcomes failed before the fix; 13 page
tests, scoped ESLint and Web typecheck passed. Ownership cleanup remains local.

## Stage 2: Notice merge ordering
**Goal**: Preserve the historical position of already persisted notices.
**Success criteria**: Multiple completed Turns retain order without duplicate
notices, while new notices and active response blocks remain visible.
**Tests**: Extend the existing notice ingestion/history regression and stream tests.
**Status**: Completed. Historical ordering failed before the fix; 84 chat/stream
tests, scoped ESLint and Web typecheck passed. Unpersisted notices remain visible;
active response merging is unchanged and no persistent deduplication state was added.

## Stage 3: Bounded batch history notices
**Goal**: Replace per-Turn notice reads with bounded page-level Store queries.
**Success criteria**: No per-Turn notice query growth; session ownership, per-Turn
limits, event ordering, and a shared high-water snapshot are preserved.
**Tests**: Real SQLite history/query-count regressions, scoped Rust tests and lint,
frontend CI and submission hooks. Update the English PR after final self-review.
**Status**: Completed. The real SQLite query-count regression failed before the fix
(3 reads for one Turn versus 5 for three) and passes with a single batch.
Service/Store: 300 tests passed; the final SQL-bound refinement passed all 8 history
tests. HTTP: 55 tests passed. Strict Clippy passed for the affected packages.
Frontend CI: 483 tests passed, with the two existing opt-in integrations skipped;
the strengthened MessageList ordering snapshot, ESLint and typecheck also passed.
Self-review confirmed all Store implementations were updated, no public wire/schema
or migration changes are needed, and no cache, duplicate DTO or new framework was added.

## Verified closure

- Durable product notices reuse the Timeline table, cursor and subscription.
  Successful results remain successful; delivery failures use Turn recovery.
- Web/Desktop/TUI display structured warnings without replacing output, reopening
  completed Turns or duplicating history. Locale switching renders ApiError again.
- Migration preserves deleted-event cursor high-water and rejects malformed
  legacy notices transactionally. Both migrations require coordinated deployment.
- Removed independent TUI deduplication state, three duplicate transcript helpers,
  unused context plumbing and duplicated historical plan narratives.
- Independent document reading resolved conflicting diagnostic severity wording.
  A controlled ablation proved that removing Web notice ingestion fails its test.
- Publication checks resolved eight file-size gate failures through private module
  extraction and reused helpers, without weakening thresholds or removing tests.
- Cached SDK credential rejection now retains the original public error and stack;
  its request-level regression failed before the fix and passes afterward.

## Final evidence

- Rust workspace: 1467 passed, 10 pre-existing ignored; no LEAK signal.
- Last affected-package run: 295 passed after final notice/TUI/lint cleanup.
  Subsequent strengthened TUI and Web notice tests passed.
- Publication cleanup: 145 SDK/provider/store tests and strict package Clippy passed;
  Desktop's 29 and Web's 370 tests passed after the behavior-preserving extraction.
- Workspace strict Clippy, package fix, formatting and git diff checks passed.
- Frontend CI: 482 passed, two opt-in integrations skipped; lint, typecheck,
  Web/Desktop builds and repository contract guards passed.
- Real rebuilt Web/Desktop acceptance: .tmp/error-contract-0h674tmq.
  Verified provider 401/500/stream errors, successful output with one usage
  warning, reload, real IPC code/class/message preservation, and one diagnostic
  with its source and captured stack. Both success-warning screenshots reviewed.
- Historical intermittent nextest LEAK was not reproduced. This is a clean
  current run, not a claimed root-cause fix or weakened detection.

## Review and release boundaries

Review the whole working-tree refactor in these units: error/source/log ownership;
Timeline/Store/migrations and completion producers; SDK/IPC/UI presentation;
generated artifacts, guardrails and documentation. Mechanical constructor/lint
migration spans many files and should be reviewed separately from lifecycle logic.
The maintenance module retains 508 non-empty lines: an advisory over the 500-line
target, below the hard limit. Its existing compaction/shutdown responsibility stays
intact; splitting that lifecycle solely to remove the advisory is out of scope.

The refactor is ready for publication as one coordinated review branch, including
the intended new source/test files, both SQL migrations and generated artifacts.
Generated bytecode is excluded. Production migration and release are not performed
as part of PR publication.
Protocol 0.4 requires coordinated backend/SDK/Web/Desktop/raw-event deployment;
take a verified SQLite backup before upgrade. See
[the error model](docs/architecture/ERROR_MODEL.md) and
[the design](spec/refactor-044-unified-error-contract/design.md) for ownership,
compatibility and rollback limits. Complete database/process outages cannot
guarantee delivery to disconnected UI.
