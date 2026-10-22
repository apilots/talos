# Logging and Sync ACK overhead

Status: implemented

## Problem and reproduction

Loro emits nine INFO diagnostics for each encoded block. Pure upload ACK handling
also reloads a document and exports the same full snapshot, although only its
acknowledged version changes. The log file has no retention policy. These are
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

Calendar-day OR size rotation uses the existing rolling-file crate through its
public RollingCondition boundary. Its built-in size condition, size accounting,
file switching, numbered archives and retention remain library-owned. The small
calendar adapter converts each supplied timestamp through the existing chrono-tz
IANA database and compares calendar dates. It does not implement file writing,
archive renaming/deletion, process locks, timer tasks or a parallel log manager.

The single root timezone config drives log timestamps and the calendar boundary.
LoggingConfig has no separate timezone setting or override. Config owns partial
startup settings and reuses RuntimeEnv plus the normal placeholder resolver for
both root timezone and logging strings, including dotenv/process precedence.
The logging crate only consumes resolved settings; Base owns its single startup
sink initialization. Server/Desktop/Worker composition passes the same root
timezone. An invalid resolved timezone fails explicitly.
The existing configuration upgrader removes the obsolete logging.timezone key
through template normalization, with the original file retained in its backup.
No runtime process changes the system timezone or global TZ environment variable.

All three maintained conf YAML templates declare rotation.max_bytes (20 MiB) and
rotation.max_files (five total files: active talos.log and talos.log.1 through .4).
Set rotation: null for a single unbounded file. Rotation occurs before the next
record after the soft size threshold, or on the next write in a different local
calendar day; no midnight polling is needed. Complete records are never split.
An existing active file is opened in append mode; its modification time seeds the
calendar condition across restart. Retention follows the library's numbered file
family, rather than a custom directory-scanning or migration algorithm. Log files
are intended to have one active appender owner; concurrent independent processes
must use separate configured files because the library does not coordinate them.

Formatted records pass through the established lossless NonBlocking worker and
WorkerGuard. No additional logging thread or cleanup task is introduced. Talos
verifies configured midnight, DST repeated hours, size rotation, retention,
restart append, queued flush, configuration/template upgrades and environment-backed
root timezone startup through the existing actual Desktop recovery scenario.

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
reports, rotation/restart, queue drain, startup/template decoding
and invalid limits. Extend the existing lost-ACK/restart scenario to reject snapshot
writes, inject cleanup failure and retain newer pending facts. Reuse existing real
Desktop stdio recovery and local multi-runtime Sync acceptance for offline metadata,
History, Live and deletion protection. Actual results belong in ignored output and
PR evidence. Cross-physical-machine and production installation are separate rollout
acceptance. Real connection faults require root-cause evidence before policy changes.
