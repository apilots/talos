# Error contract

Owner: `talos-error`. See [refactor-044](../../spec/refactor-044-unified-error-contract/design.md) for rationale.

## One model, two categories

One internal `TalosError` carries one public `ApiError`, private context, the original typed source, capture location and a shared native backtrace. `TalosResult<T>` is Rust's ordinary Result.

- Business: known product meaning (validation, permissions, missing data, conflicts, limits, recognized provider rejection).
- Internal: unclassified technical failure, malformed response, broken invariant or unexpected task failure. `InternalError` identifies this category; every other code is Business.

Both categories are visible and reported. Category does not determine retryability or protocol status. Rust does not automatically convert arbitrary errors or panics into responses. There is no separate category enum because it would only duplicate `ErrorCode`.

## Construction and propagation

`TalosError::new(code, message)` takes a reviewed English explanation. `internal` requires static text; `from_source` / `internal_with_source` retain a new dependency cause. `with_source` attaches the original cause during construction. `context` and clone preserve public text, code, source and original capture. Never stringify/re-wrap an existing TalosError.

Ordinary Agent APIs return `TalosResult` directly. `AgentRunFailure` temporarily composes the original `TalosError` with operation-owned retry policy only while publishing or recovering a terminal Turn. Finished settlements retain the original in-process Result alongside the public event, avoiding reconstruction from JSON.

Agent, filesystem, and file-upload callers use `ErrorCode` directly; do not add
parallel kind enums that merely map to the same codes. Memory recovery branches
on its owned code. Structured tool classification belongs to `ToolDispatchFailure`
and is consumed when building the tool outcome; neither concern adds optional
metadata to `TalosError`.

## Public contract

```json
{"status":"error","error":{"code":"providerUnavailable","message":"Model \"alpha\" is not configured for provider \"local\".","localization":{"type":"modelNotConfigured","provider":"local","model":"alpha"}}}
```

Message is the specific reviewed English reason, not automatically the code's generic heading. Unknown technical causes still explain the failed operation and what can safely be disclosed. Never put credentials, server paths, SQL, or remote response bodies in public text. User-requested resource names may be included.

Code and typed descriptor parameters determine program behavior. Message is display
text, never a discriminator, retry condition, or decoder checksum. A wording change
does not change validity. The backend renders its English template once; consumers
must not duplicate that template to compare incoming sentences.

Localization is required and nullable. The generated ErrorLocalization union currently contains modelNotConfigured and attachmentCountExceeded. No arbitrary translation keys, maps or diagnostic details bags. Messages are bounded to 4096 Unicode scalars, string parameters to 256, and integer parameters to nonnegative JavaScript-safe integers. Invalid wire data fails visibly. Oversized constructed messages carry an explicit truncation marker; invalid construction parameters omit localization, not the original cause.

HTTP and JSON-RPC application responses use ApiResponse. JSON-RPC framing errors retain protocol numeric codes. Failed events/history reuse `AgentTurnFailed { error: ApiError, retryable: bool }`; retryability belongs to the Turn. Framework rejection middleware uses the same projection. After SSE headers, failures use events/termination, never another HTTP response.

HTTP session, turn, recovery, and project handlers preserve source-owned codes;
they do not reconstruct errors from a route-specific heading. Timeline cursor
classification belongs to the timeline service, so RPC and HTTP agree. Multipart
transport failures carry TalosError directly, including typed parser sources.

Framework response normalization preserves HTTP status, Allow and Retry-After;
replacing the body only invalidates representation headers. SDK JSON parsing
retains its diagnostic cause without exposing response excerpts. Uncorrelated
protocol errors reject pending requests; Desktop transport failures use existing
subscription termination notifications, not an unconsumed runtime-only status.
Keep lagged termination typed so recovery can distinguish it from interruption.
Decode untrusted ApiError payloads once at transport/event ingress. TalosApiError
only carries the resulting typed value; wrapping a parsed HTTP/auth failure must
not repeat validation in its constructor.

Electron main returns this same plain envelope. Preload forwards it; renderer SDK reconstructs TalosApiError. Never send native Error objects as IPC error payloads. Concurrent initialization shares one handshake. Bootstrap connection failure must not masquerade as logout or discard valid credentials.

## Presentation

- Branch on code, not text; render through apiErrorMessage/userVisibleErrorMessage.
- Keep operation and terminal errors structured in component state/live history;
  translate during rendering, including queue, attachment, dialog, and command failures.
- Translate typed descriptors, never replace specific reasons with code headings.
- Missing translations preserve backend English. Render as text, not injected HTML.
- Show failed turns even when no assistant text exists. Recovery/reload must preserve the reason.
- TUI/CLI use `to_api_error().message`, never diagnostic Display.

## Reporting ownership

Product completion notices share the existing Timeline log, global cursor and
subscription. TimelineEventPayload distinguishes Agent facts from SystemNotice;
only Agent facts carry execution sequence numbers or enter continuation reducers.
Notice severity is presentation policy, not a third error category. SystemNotice
content retains either informational text or ApiError, never two competing
messages. Session history exposes product notices separately from the canonical
Turn result. Successful work remains successful when an independent effect fails.
Turn-limited subscriptions deliberately end at the terminal fact; interactive
clients use Session subscriptions for subsequent notices. A storage outage can
prevent durable publication and must be reported separately, never assigned a
fabricated replay cursor.

The durable log resolves publication order between independent writers before
advancing subscription cursors. Web notices have stable event IDs and remain
outside collapsed activity; locale changes render their retained ApiError again.
When history contains a notice ID, Web keeps that historical position instead of
appending its live duplicate after newer Turns. Unpersisted live notices remain visible.
TUI reuses the Session subscription that already carries queue changes. Its
transcript blocks own replay identity, so there is no separate notice registry.
Notices are not injected into Provider continuation state. A history read rejects
more than 1024 notices for one Turn explicitly instead of silently truncating them.
History batches notices for the returned Turn identities (up to 1000) at one
captured high water, excluding pagination lookahead. Store queries enforce exact
Session/Turn ownership and bounded results; no per-Turn notice query or cache is used.

Use `talos_logging::report_error` once at the handling boundary:

| Boundary | Owner |
| --- | --- |
| HTTP / RPC request | Adapter projecting the original error |
| Background Turn / recovery | Service completion |
| Request-owned compaction | Request adapter |
| Failure converted to tool result | Agent kernel execution |
| Failed MCP startup retained as status | MCP runtime |
| Process startup / automation / tasks | Existing process or task owner |

Propagation and historical replay do not log again. Report otherwise-lost secondary cleanup errors separately. No logged flag, deduplication registry or error bus. Business uses WARN; Internal uses ERROR. File logging applies backpressure and retains its flushing guard through shutdown.

Reports include code/text, private context, available sources, location and captured native stack. Capture happens at initial conversion, not necessarily the dependency's throw site; it is not a logical async stack or remote stack. Source chains are bounded/cycle-checked with explicit truncation. Adapters remove sensitive remote bodies/URLs before reporting. Logs require access controls; no string-based system can prove all arbitrary diagnostic text secret-free. Abort/unobservable lost tasks cannot be promised a client response or invented stack.

Explicit Result discard is limited to documented ownership transitions such as delivery to an already-cancelled receiver. Do not discard operational or cleanup failures.

## Enforcement and extension

Workspace lints deny unused must-use Results. Type-aware frontend ESLint rejects direct Error/ApiError.message presentation outside the presenter. Repository checks reject retired abstractions and known diagnostic serialization paths; positive/negative fixtures validate them. These checks supplement, not replace, source/disclosure, request/event/history, migration, snapshot and real-path tests.

The same guards reject known prose-based decisions in Rust/SDK/smoke code and
directly storing presenter output in error setters. They are targeted checks, not
a complete data-flow proof. Start invalid-payload fixtures from a valid object and
change one condition at a time; removing its validator must make that case fail.
Smoke runners preserve application codes through CDP instead of parsing exception text.

For a new failure: classify it, preserve the source, write specific safe English, identify its report owner, and test request/stream/history behavior. Add localization only for a real translated scenario and cover both languages and parameter validation. Regenerate protocol artifacts and review consumers together.

Run `just gen-app-protocol`, scoped `just test -p <package>`, `just fmt`, `just fix -p <package>`, frontend lint/test/typecheck, and `pnpm check:repo`. Real Web/Desktop acceptance uses conf/talos-dev.yaml with isolated data and local provider overrides; see talos-app/scripts/error-contract-e2e.py.

## Upgrade and rollback

Protocol **0.4** changes errors, failed-event retry nesting, and TimelineEvent.event
to the tagged union `{type: "agent" | "systemNotice", data: ...}`. SessionHistoryTurn
adds notices; AgentSystemNotice uses tagged Text/ApiError content instead of a
message field. RPC adds timeline/systemNotice. Generated clients/server reject
older handshakes. Upgrade backend, SDKs, Web, Desktop and raw-event consumers
together; HTTP clients also require coordinated deployment. TUI GatewayClient
implementors migrate the queue-only subscription to subscribe_session_events.
No legacy wire guessing or compatibility alias.

SQLx migration `202609060001_unified_error.sql` upgrades canonical agent_events.payload_json transactionally through _sqlx_migrations. Identity/order/code/retry metadata remain intact. It adds nullable localization, bounds text, and replaces legacy Internal diagnostic text with an explicit withheld-details message. Invalid legacy payloads roll back and do not mark completion. Archived legacy_timeline_events is not decoded by current history.

Migration `202609060002_timeline_notices.sql` makes Agent sequence nullable only
for independent product notices, upgrades existing SystemNotice blocks and
preserves event IDs, deleted-event cursor high-water, indexes and Session cleanup.
Malformed legacy notice content aborts the transaction without marking the version.

Before release, stop writers and create an access-controlled consistent SQLite backup (backup API or VACUUM INTO a new file). Verify it opens; retain matching old binaries/configuration. Rollback stops writers and restores that backup with the old binaries. This restores the backup's recovery point and loses subsequent writes; export any required newer data before rollback. Reverting binaries alone or reverse-rewriting JSON is unsafe. Tests cover repeat application, atomic rejection, and file-backed backup restoration with the old ledger.
