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

Daily rotation and retention use the existing tracing-appender RollingFileAppender,
configured through its native Rotation::DAILY and max_log_files builder options.
The default is five daily files in UTC, named talos.log.daily.YYYY-MM-DD. The
separate daily prefix keeps framework retention away from the previous talos.log
and other log families. No legacy log bytes are moved, overwritten or removed.

All three maintained conf YAML templates declare rotation.max_files. Set
rotation: null for the existing single unbounded output file. Daily rotation does
not impose a byte-size limit; the earlier custom 20 MiB policy and its hand-written
rotation/file-lock implementation are removed. Framework rotation writes whole
formatted records through the existing lossless NonBlocking worker and WorkerGuard.
No additional thread, manager, filesystem lock, archive numbering or dependency is
introduced. Framework scheduling/retention is covered by its upstream tests;
Talos verifies configuration, retention integration, restart append and queued flush.

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
