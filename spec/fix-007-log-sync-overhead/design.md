# Logging and Sync ACK overhead

Status: implemented

## Problem and reproduction

Loro emits nine INFO diagnostics for each encoded block. Pure upload ACK handling
also reloads a document and exports the same full snapshot, although only its
acknowledged version changes. The log file has no size retention. These are
separate from genuine Sync and Worker connectivity errors.

Reproduce the ACK overhead by exporting a pending resource and acknowledging the
returned cut. A SQLite trigger rejecting updates to snapshot_blob, version_vector
or generation exposes the redundant document write. Repeated snapshot diagnostics
can also be observed while normal document changes are exported.

## Logging policy

The existing LoggingConfig owns target_levels, a map of tracing targets to typed
off/error/warn/info/debug/trace thresholds, and nullable rotation. Defaults suppress
only loro_internal::oplog::change_store::block_encode below WARN. Explicit target
configuration enables codec diagnostics. Product Business WARN/Internal ERROR,
typed sources and captured stacks retain their existing reporting contract.

Rotation defaults to 20 MiB and five total log files, including the active file.
A single oversized record stays whole and may exceed the soft limit. All three
maintained conf YAML templates declare these defaults. Set rotation: null for the
existing unbounded policy. Use tail -F to follow pathname changes.

Size rotation runs on the existing lossless tracing_appender worker. Cooperating
current-version processes lock talos.log.lock, reopen the active pathname while
locked and write complete event buffers. Numeric archives retain chronological
order. Rotation archives the old file before accepting the new record, then prunes
the oldest archives. Opening a writer also applies retention. Process exit releases
OS locks and descriptors. The small lock file stays in place to coordinate writers;
it is not a retained log. No extra thread, manager, timer or dependency is added.
Older uncoordinated binaries should be stopped before enabling shared-file rotation.

## ACK persistence

The existing Sync facade verifies the scope and exported cut against the durable
current version. Its Store reads only version and ACK vectors, never the snapshot
blob. The same transaction updates only acknowledged_vector and
acknowledged_generation, then removes confirmed exported outbox rows. A missing
outbox cut is an already committed ACK; an existing outbox row without its document
is broken state and fails explicitly.

Original snapshot/version, later pending changes, duplicate/lost ACK retry and
restart semantics remain intact. A cleanup failure rolls back the ACK and outbox
together. No database migration, SDK protocol or Sync Server change is required.

## Connection diagnostics and boundaries

Existing handling owners record typed phase/operation/wake reasons, original error
codes and elapsed time. The account supervisor distinguishes account verification,
local settings, H restoration and Worker/binding reconciliation. The H runner
distinguishes connect, catch-up, feed, download, upload and local state work; its
endpoint field contains only the URL origin. The Worker retry owner reports the
original error source under its operation span. Terminal errors continue to their
existing handling owner, so propagation does not report them twice.

No diagnostic triggers remote requests. Existing reconnect ownership, keepalive,
business heartbeat, lease renewal and fixed five-second temporary registration
retry remain unchanged. No History pagination, durable grant, protocol, attachment
or real-profile configuration change belongs to this work.

## Verification strategy and limits

Use the owning regression suites for explicit diagnostic opt-in, retained error
reports, rotation/restart/independent writers, queue drain, startup/template decoding
and invalid limits. Extend the existing lost-ACK/restart scenario to reject snapshot
writes, inject cleanup failure and retain newer pending facts. Reuse existing real
Desktop stdio recovery and local multi-runtime Sync acceptance for offline metadata,
History, Live and deletion protection. Actual results belong in ignored output and
PR evidence. Cross-physical-machine and production installation are separate rollout
acceptance. Real connection faults require root-cause evidence before policy changes.
