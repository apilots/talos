# Unified Error Contract

Status: implemented and verified.
The current error model is authoritative for implemented APIs and ownership.
Owner: Talos error, logging, gateway, SDK, and presentation boundaries.
Baseline: repository inspected on 2026-09-06.
Related: [current error model](../../docs/architecture/ERROR_MODEL.md).

## 1. Decision and scope

Keep one internal `TalosError`, one `TalosResult<T>`, and one public `ApiError`.
Classify failures as `Business` or `Internal`. Remove the `Public` category.
Both categories produce visible client feedback and diagnostic logs. Category
does not determine whether a failure is visible, retryable, or worth logging.

Business failures have a known product meaning: invalid configuration, rejected
input, missing resources, authentication, permission, state conflicts, limits,
and recognized dependency failures. Internal failures are unknown or unexpected
technical failures: database faults, malformed provider responses, broken
invariants, and unexpected task termination. Classify at the boundary that knows
the meaning: a recognized provider rate limit is Business; an unrecognized
provider protocol failure is Internal.

Talos-authored public messages and diagnostic context are English. Business
messages explain the specific failure. Internal messages explain the operation
and the available, disclosable cause. Neither is automatically replaced with a
fixed sentence selected by its error code.

This proposal covers error construction, source preservation, logging, HTTP,
JSON-RPC, Desktop IPC, SDKs, terminal events, recovery/history, Web/Desktop/TUI/CLI,
startup, background work, and the error boundaries around tools and MCP.
It does not redesign successful tool results, implement automatic retries, add
an error database, or introduce an error manager, registry, plugin system, or bus.

### 1.1 Rust ecosystem basis

Rust provides `Result` propagation and panic handling, not automatic global
exception-to-response conversion. Product classification is a layer above those
mechanisms; Business/Internal must not be equated with Result/panic. An ordinary
`Err` can represent an Internal failure. [Rust Book](https://doc.rust-lang.org/book/ch09-00-error-handling.html)

Use the existing `thiserror` dependency for standard Error/source implementations
where it removes boilerplate. It supports opaque public error wrappers and
private representations; it does not define a logging or UI policy. The
repository currently uses thiserror 1; adopting this design does not require a
major dependency upgrade. Avoid relying on nightly automatic Backtrace/provider
integration; stable standard-library capture can remain in private diagnostic
state. [thiserror documentation](https://docs.rs/thiserror/latest/thiserror/)

Borrow anyhow's context-preserving approach: attaching an operation must retain
the underlying error and its identity. Existing CLI/application integrations may
keep anyhow; do not replace typed service contracts with anyhow everywhere or
stringify an existing anyhow chain when entering TalosError. No new dependency
is necessary just to imitate its API. [anyhow Context](https://docs.rs/anyhow/latest/anyhow/trait.Context.html)

Axum turns handler errors implementing IntoResponse into HTTP responses, but
fallible middleware/services and extractor rejections need their own boundary
handling. This supports a shared public projection in gateway adapters rather
than a global catch-all framework. The repository uses Axum 0.7; validate exact
layer APIs against that installed version before implementing guidance from
current documentation. [Axum error handling](https://docs.rs/axum/latest/axum/error_handling/index.html)

Tokio exposes task panics and cancellation through JoinError, independently of
the task's returned Result. Existing supervisors must observe both outcomes.
Dropping the handle is not error reporting. [Tokio tasks](https://docs.rs/tokio/latest/tokio/task/index.html)

Native stacks and async diagnostic context are complementary. Reuse tracing
spans and identity now; tracing-error's SpanTrace is an available future option
only if an identified diagnostic gap requires it. Do not add it speculatively.
[tracing-error documentation](https://docs.rs/tracing-error/latest/tracing_error/)

These sources establish runtime/library behavior. The taxonomy, public payload,
mandatory Business backtraces, and translation policy below are Talos product
decisions, not claims that the ecosystem prescribes one universal framework.

## 2. Historical baseline: failures addressed by this refactor

The table below describes the pre-refactor implementation, not current defects.
Implemented behavior and extension rules live in the current error contract.

| Current implementation | Consequence |
| --- | --- |
| `talos-error::public(code, diagnostic)` creates a default `ApiError` | Specific business explanations never enter the public response |
| HTTP reconstructs `ApiError` using only its code | Public content would be lost even after extending the source error |
| Agent converts `TalosError` into categories and back | Limit/authentication/file semantics and source information can change or disappear |
| Harness `settle_failure` publishes `AgentError.message` | Internal diagnostic strings enter canonical events and persisted history |
| Timeline recovery constructs messages from persistence diagnostics | A second public projection policy bypasses the error boundary |
| `ApiError` and `AgentErrorInfo` independently define code/message | Request errors and terminal failures have different contracts |
| Desktop main throws SDK exceptions through Electron IPC | Renderer loses custom error properties and class identity |
| Web translates by broad code; TUI prints event message | Web hides specific causes while TUI exposes different content |
| Error model stores caller location but no captured backtrace | Logging a formatted error cannot provide the promised failure stack |
| Boundaries log only `is_internal()`; sources are often stringified | Some failures lack diagnostics, while others are logged repeatedly |

Primary implementation locations are `crates/infra/error`, `crates/infra/logging`,
`crates/orchestration/agent/src/error.rs`, `agent/src/harness/settlement.rs`,
`agent-api/src/event.rs`, `service/src/turn/agent_execution/timeline_writer.rs`,
and both gateway adapters under `talos-rs`; Desktop IPC, SDK adapters, and
`web/src/shared/i18n/apiErrorMessage.ts` under `talos-app`.

## 3. Internal model and ownership

### 3.1 One error value

`TalosError` is a non-serializable value with private state. Its state contains:

- The complete public `ApiError` described below.
- Internal operation/context, the original typed source, first-capture location,
  and first-capture backtrace.
- No optional domain metadata bags; owning domains carry non-product execution metadata.

Store diagnostics once in shared immutable ownership so cloning an error across
Agent tasks retains the same source and backtrace. Context additions preserve
the original failure; they must not manufacture a new root error from `Display`.
Backtrace/source values are not part of business equality or wire serialization.

Derive the two categories directly: `InternalError` is Internal and every other
recognized code is Business. Do not expose or store a second category enum; it
would duplicate `ErrorCode` and permit contradictory state. New unknown technical
failures enter through `InternalError`, while new business meanings add a stable
code.

Constructors have explicit purposes:

- A business constructor takes a stable code and a reviewed English explanation.
- A localized business constructor takes a typed message descriptor and derives
  its code and English rendering from that descriptor.
- An internal constructor takes operation context and a disclosable English
  cause; a source-taking form retains the original error separately.
- A contextual wrapping method preserves code, public explanation, source,
  diagnostics, and capture identity unless the owning domain deliberately makes
  a documented semantic translation.

These are conceptual API contracts; names and exact Rust signatures are finalized
within the existing crate during implementation. They do not require builders,
factory traits, or separate BusinessError/InternalError class hierarchies.

### 3.2 Layer responsibilities

| Owner | Required responsibility | Excluded responsibility |
| --- | --- | --- |
| `talos-error` | Error model, stable codes, controlled public construction, pure projection | Logging sinks, transport codes, UI components |
| Existing domain/provider adapters | Recognize failure meaning; choose disclosable cause; preserve source | Localized copy, response envelopes |
| Agent framework | Return `TalosResult`; compose retry safety only at terminal settlement | Duplicate or reinterpret already established product codes |
| `talos-logging` | Common diagnostic reporting through existing tracing infrastructure | Business classification, protocol serialization |
| Gateway/terminal owner | Report the handled failure and serialize its public projection | Rebuild messages or discard fields |
| SDK/IPC adapters | Validate and preserve public errors; identify local transport/protocol failures | Translate messages or expose backend diagnostic objects |
| Presentation | Render the full public failure and optional localization | Derive semantics by parsing English messages |

Ordinary Agent APIs propagate `TalosError` directly. `AgentRunFailure` adds only
operation-owned retry safety at terminal settlement and recovery boundaries.
Lifecycle consumers inspect the product code; a parallel Agent error or kind enum
would duplicate classification and permit inconsistent state. Known product codes,
sources, and capture identity survive Agent task propagation unchanged. Tool
classification remains in the tool execution domain and is consumed into the
existing Function/MCP output envelope rather than stored on `TalosError`.

### 3.3 Unclassified failures and no silent error exits

Business and Internal exhaust product failures. An error without a known business
mapping enters Internal, preserving its source and the failed operation. This is
failure classification, never a fallback to success or to an empty explanation.

An existing TalosError is propagated as-is. A dependency adapter first recognizes
known actionable outcomes and wraps everything else as Internal. Generic
normalization is confined to real application/dependency boundaries; do not add a
blanket `From<E>` that reclassifies known Talos errors, conflicts with identity
conversion, or makes every library depend on Talos business types.

| Exit path | Required owner and behavior |
| --- | --- |
| Unknown std/library/anyhow error returned through Result | Dependency adapter retains source and operation, classifies Internal |
| Existing Business/Internal error | Propagate without reclassification, stringification, or recapture |
| Handler/extractor rejection | Request boundary returns a structured application or typed protocol rejection |
| Middleware/service timeout/error | Existing gateway layer maps known timeout semantics; unknown failure becomes Internal |
| Unwinding panic in an owned request/task | Capture available panic diagnostics; fail that operation through its supervisor; do not resume partially mutated business work |
| Expected user cancellation | Preserve normal cancellation lifecycle or existing Cancelled code; do not call it unknown Internal |
| Unexpected task/channel disappearance | Supervisor emits an Internal or already-defined explicit lifecycle failure, with cause/context |
| SSE failure after response headers | Use the terminal event when deliverable; HTTP status cannot be changed retroactively |
| Response/event delivery fails | Report server-side; client transport reports connection loss; never promise the original cause crossed a broken channel |
| Abort, kill, OOM, disconnected process | Existing process/connection supervision shows service loss and recovery state; unavailable server diagnostics are acknowledged |

`catch_unwind` is limited to deliberate unwind-safe isolation boundaries and
cannot catch process-aborting panics. It is not an automatic replacement for
Result. Review recovery and shared-state consistency before intercepting panics;
if continuation is unsafe, retain process termination and client connection-loss
feedback. [Rust catch_unwind](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html)

Audit discarded fallible values (`let _ =`, `.ok()`, default substitution,
detached tasks), but do not ban those tokens indiscriminately. Every intentional
discard must have an established lifecycle explanation and reporting owner, such
as a response receiver that has already disconnected. Enforce Rust unused-Result
warnings in CI; separately test supervisors and known explicit-discard sites.

## 4. Public contract

The proposed current-version wire shape is:

```typescript
type ApiError = {
  code: ErrorCode;
  message: string;
  localization: ErrorLocalization | null;
};
```

`code` is the stable machine-readable outcome. `message` is the complete public
English explanation, not a generic label and not an arbitrary diagnostic dump.
`localization` is an optional exact message descriptor, introduced only for
messages that the existing multilingual UI can actually render.

An initial, small descriptor set covers concrete existing needs:

```typescript
type ErrorLocalization =
  | { type: "modelNotConfigured"; provider: string; model: string }
  | { type: "attachmentCountExceeded"; limit: number; actual: number };
```

The descriptors belong to the shared error contract and generate Rust schema/TS
types through existing tooling. ModelNotConfigured maps to the existing
ProviderUnavailable code; AttachmentCountExceeded maps to AttachmentLimitExceeded.
One backend rendering implementation produces the English message from the same
typed descriptor. Additional configuration-validation descriptors are added only
when the concrete validation rule, typed parameters, and translations ship together.
Other specific business messages and all initially unsupported internal causes use
`localization: null`. No free-form localization keys, parameter maps, or reserved
variants are introduced. A descriptor is never required merely to display an error.

English wording is never validated by comparing sentences. Decoders validate the
code, descriptor shape and parameter bounds, not the backend's English template.
Classification, retry and recovery branch on codes or typed state, never on prose.

Public constructors and SDK decoders enforce nonempty messages, descriptor/code
consistency, and bounded fields. Proposed display bounds are 4,096 Unicode scalar
values for message, 256 for each descriptor string, and safe nonnegative integers
for count parameters. Oversized public descriptions get an explicit truncation
marker; invalid wire values produce a visible protocol failure instead of a
silently changed valid error. Internal diagnostics retain the available cause.

Construction must not fail while reporting the original failure. If an otherwise
valid business descriptor exceeds its parameter bounds, emit `localization: null`
and the bounded, explicitly marked English explanation. Never truncate a model
identifier or numeric parameter inside a descriptor and thereby change meaning.
An over-limit descriptor received over the wire is still a protocol violation.

Do not add an independent wire kind, generic details object, arbitrary actions,
global retry flag, or mandatory new error ID. Use existing request/turn/session
identity where available. No error lookup API is introduced.

## 5. Internal exceptions remain visible

Examples of the intended backend messages:

| Failure | Kind | Visible English message |
| --- | --- | --- |
| Selected model is absent from configuration | Business | `Model "model-a" is not configured for provider "provider-a".` |
| Too many attachments | Business | `This request contains 6 attachments; the limit is 4.` |
| Configuration validation fails | Business | `The configured timeout must be greater than zero.` |
| Database is locked while reading a session | Internal | `Failed to load session: database is locked.` |
| Provider returns malformed JSON | Internal | `Failed to read the model response: invalid JSON at line 1, column 24.` |
| Unexpected task termination with no recoverable cause | Internal | `The model execution task stopped unexpectedly; no further cause was available.` |

Do not promise a root cause the application did not receive. If the cause cannot
be disclosed, state the failed operation and that technical details were withheld;
do not silently erase the explanation or return success.

Public cause selection occurs in the adapter that understands the dependency.
For example, a SQL adapter can expose a recognized lock reason while retaining
the original database exception for diagnostics. Unknown dependency errors must
not expose raw bodies, credentials, SQL, absolute server paths, or arbitrary
`source.to_string()` text. String replacement of known secrets alone is not a
general disclosure policy. Share domain-specific projections at the dependency
boundary, rather than repeating them in HTTP, events, and UI.

Redact secrets from logs as well. Full diagnostics means the complete available
exception structure and captured stack, not unrestricted request/response dumps.
Third-party human-readable content may be non-English; Talos must not pretend it
has translated it. Talos-authored summaries remain English, and external tool
output remains explicitly external content.

## 6. Diagnostic capture and reporting

Every failure represented by `TalosError`, including Business, captures the
available diagnostic stack at construction or first wrapping. Capturing a stack
only when responding to HTTP is too late. Preserve an original source-provided
trace when available and label a first-wrapper trace as such.

Use explicit capture policy for this product requirement: ordinary
`Backtrace::capture` can be disabled by environment configuration, whereas
`Backtrace::force_capture` requests capture independently of those variables.
Capture once per original Talos failure and retain its status; do not change
process environment inside library code. Benchmark the cost on the failure path
and report unsupported capture honestly. [Rust Backtrace](https://doc.rust-lang.org/std/backtrace/struct.Backtrace.html)

For Rust async work, a captured native backtrace is not the full logical async
call graph. Combine it with preserved source chains, operation context, and
existing tracing/request/turn identity. Do not claim remote provider or MCP
stacks when the remote service did not supply them.

Reuse `talos-logging` for a common report operation. Each report includes category,
code, public explanation, internal context, all available source-chain entries,
capture location, captured stack, and existing correlation fields. Log capture
is required for both categories: report_error uses WARN for Business and ERROR
for Internal, including failures that later recover. Additional retry-progress
tracing may use lower levels but never replaces the required diagnostic report
or removes its stack/context fields. UI notice severity is independent.
The existing logging configuration must keep the shared diagnostic reporting
target enabled independently of ordinary application verbosity; filtering WARN
must not silently remove required Business reports.

The boundary that handles a failure reports it:

- HTTP/RPC request owner before returning an error response.
- Agent terminal owner before publishing/committing a failed terminal event.
- Background task supervisor or startup owner for failures without a request.
- Local Desktop/SDK owner for failures that occur before a backend response exists.

Propagation does not log again. Terminal history replay does not re-report an old
failure. Recovered retry attempts are recorded with attempt context; final failure
is separately identified. Multiple observations of one outcome do not justify
creating a global deduplication registry or a mutable `logged` flag on the error.
For background turns, the terminal owner reports the original diagnostic once;
requests relaying the recorded public outcome do not report it again.
Request-owned compaction instead returns its original Result to the request
adapter, which owns reporting even when a terminal event is also published.
Early rejection also belongs to the request boundary.
Preserve the original in-process diagnostic until its reporting owner
has consumed it, rather than reconstructing it from the public event.

Wire/history errors are not converted back into freshly captured backend errors.
They contain only public information, and original stacks cannot be recreated.
Rust SDK remote-error handling must make that distinction explicit. Remote
failures may add a local request context, but it must not be called the server stack.

Owned task panics/JoinErrors and unhandled startup failures must feed the same
reporting policy where the runtime can intercept them, preserving existing
termination/cleanup semantics. Panic hooks and task supervisors need one agreed
reporting owner, not duplicate stack dumps. OOM, abrupt process kill, unavailable
symbols, and failed log storage have explicit diagnostic limitations; they cannot
be made into guaranteed server stack reports by this abstraction.

Audit the existing nonblocking logging sink: queue saturation must not silently
drop exception reports, and normal shutdown must retain/drain its guard. Use
existing sink configuration and visible failure reporting; do not build a new
log spool or storage service. Secret handling, source-chain cycle protection,
and any size limits must retain a visible omission marker, never silently claim
that incomplete diagnostics are complete.

## 7. Request, event, IPC, and SDK contract

HTTP preserves the complete `ApiError` and selects only HTTP status. JSON-RPC
retains its existing distinction between protocol errors and application errors;
application errors continue using the shared application envelope. Numeric
JSON-RPC parse/method errors remain transport concepts and get an explicit local
SDK presentation path rather than being confused with business codes.
Normalizing framework responses preserves their HTTP status and transport headers
(including Allow and Retry-After); only the replaced body's representation metadata
is removed. Protocol/input owners report rejections once, separately from the
already-reported application envelope.

Replace duplicated `AgentErrorInfo` with shared `ApiError`. Retain the existing
runtime-owned retry decision as a sibling on the failed-turn payload:

```typescript
type AgentTurnFailed = {
  error: ApiError;
  retryable: boolean;
};
```

The Harness and service recovery paths both use the same pure public projection.
Events, history, subscription recovery, and SDK consumers preserve the object.
Cancellation/normal completion remains a normal lifecycle outcome when it already
is one; this proposal does not convert all control flow into exceptions.

Desktop main catches application errors and returns the existing application
result envelope containing their serializable `ApiError`. Renderer-side SDK
decodes it and reconstructs `TalosApiError`. Do not throw custom error instances
through Electron and expect their extra properties to survive. Avoid double
wrapping or double decoding by keeping exactly one envelope at the IPC boundary.

Local main-process/internal errors get an explicit, logged public projection.
Protocol errors keep their existing typed transport envelope; the IPC adapter
must carry that branch explicitly where needed. A disconnected IPC channel or
crashed main process cannot return an envelope; renderer transport handling must
still produce a visible connection failure. Browser network failures, stream
termination, and invalid response decoding also reach that presentation boundary.
JSON parsing uses one SDK boundary that retains parser causes without displaying
response excerpts. Uncorrelated protocol failures reject pending requests, and
Desktop transport failures terminate active subscriptions through the existing
interrupted notification, including events arriving before subscribe acknowledgement.
Lagged subscriptions retain their typed recovery reason. Background reconnect
failures are reported and observed even when no request is awaiting them.

SDKs preserve public messages and descriptors. Malformed/unknown codes remain
explicit protocol failures under the current strict contract. Missing translation
is different from an invalid protocol and must not cause request failure.
Rust and TypeScript decoders apply the same rule: an unknown localization tag,
invalid known descriptor parameters, or descriptor/code mismatch is a visible
protocol failure. A valid known descriptor without a translation is not. This
proposal keeps a closed generated union; it does not promise forward-compatible
decoding of unknown variants.

## 8. Frontend behavior and internationalization

Use one presentation entry point across Web/Desktop. Store structured public
failures in application state; format them during rendering so a locale change
does not leave stale, previously translated error strings in the store.

Rendering policy:

1. A supported typed localization descriptor produces an exact business message.
2. A null descriptor or missing translation for a valid known descriptor displays
   the backend English `message`.
3. Internal failures always retain their available public cause. A localized
   category heading may accompany it, but cannot replace it.
4. Native/transport exceptions are normalized at the SDK/client boundary with
   operation context and the available disclosable cause; they never disappear.

Use the existing i18n resources for category headings and typed business
templates. Do not parse English sentences for translation, use broad code
translations to replace specific descriptions, or send backend locale settings.
An English-only client can always display `message` without i18n support.

Field validation stays inline, failed operations stay near their operation,
failed turns stay in the conversation, and disconnection stays in connection UI.
Reuse existing components; one formatter does not require one global toast or
an additional notification service. Render text safely, allow reading/copying
the public explanation, and avoid exposing a full backend stack in the UI.

TUI and CLI use the same public English explanation. They do not use the internal
`TalosError::Display` as a user-facing presentation API. Existing status/exit-code
semantics remain separately defined. Runtime error panels and plugin/MCP status
warnings must use reviewed public descriptions as well.

## 9. Tools, MCP, and normal control flow

### Completion warnings

Reuse the existing Timeline table, cursor and broadcast for independent completion
effects. TimelineEvent.event is a tagged Agent/SystemNotice union; only Agent
events carry canonical execution sequence numbers. A product notice carries
Turn identity, occurrence time, severity and SystemNoticeContent (Text or ApiError).
The same SystemNoticeContent also replaces the former message field on Agent
notice blocks. Severity is presentation policy, not another error classification.

Usage persistence, title generation, memory reflection and completion cleanup
report the original error once, then commit a public notice before broadcasting.
Input delivery failure instead enters existing failed-execution recovery because
the accepted operation did not complete. Recovery/task-channel failures use the
same notice owner if no terminal settlement can be returned. Independent notices
never modify the canonical result, continuation sequence or active-Turn state.

SessionHistoryTurn exposes notices separately from AgentRunResult. Interactive
Web/Desktop/TUI keep Session subscriptions after a Turn terminates; explicitly
Turn-limited CLI/SDK consumers end at the terminal fact and must use a Session
subscription or history to observe later product effects. No extra event bus,
notification manager, warning database or duplicate error object is introduced.
History fetches notices in one bounded Store batch for the returned page, not for
the lookahead Turn. Exact Session/Turn identities and one captured high water
preserve ownership and replay boundaries. The batch accepts at most 1000 Turns
and fails explicitly above 1024 notices per Turn. Web prefers historical notice
positions for matching event IDs while retaining notices not yet present in history.
Public subscribers drain the durable log when independent writers publish out of
order. Failed durable publication produces a separate diagnostic, never a made-up
cursor or a promise that database/process outages always reach disconnected UI.

Tool outputs have a different owner and purpose from failed API requests: the
model and user may need stdout/stderr or a rejected command explanation to act.
Keep the existing tool result envelope and result-kind metadata. A command exit
failure is not automatically an unknown Talos exception or a failed whole turn.

When an actual Talos exception crosses into tool execution, report it and project
its public explanation through the same policy before incorporating it into the
tool result. Preserve existing output bounds and model-context limits. Do not
inject full diagnostic sources or backtraces into model context.

MCP initialization/transport/schema failures follow the same rule; partial server
availability remains explicitly represented, not silently successful. MCP error
codes are interpreted at the existing connection/tool boundary. Remove string
parsing of formatted MCP error codes once the typed source reaches that boundary.
Do not build another tool error taxonomy or reconnect/reset framework here.

## 10. Mechanical and semantic enforcement

| Invariant | Code/CI enforcement |
| --- | --- |
| One authoritative public object | Generated schema/TS reuse; remove `AgentErrorInfo`; private internal state and explicit exports |
| No request/event message loss | Shared fixture round trips across HTTP, RPC, IPC, terminal event, recovery, and history |
| No product-code degradation | Agent adapter round-trip integration tests, including permission, authentication, budget, and attachment failures |
| No diagnostic source loss | Injected chained errors retain source and original capture through async Agent paths |
| Both categories visible and logged | Captured-log tests plus UI/TUI snapshots and real transport integration tests |
| No ad hoc frontend rendering | Type-aware ESLint restrictions on exception message rendering and SDK-boundary imports; enforce migration with no growing allowlist |
| Safe public projection | Tests with credential/path/SQL/body sentinels and representative dependency failures |
| Exact localization | Generated typed descriptor coverage and parameter checks; English fallback and locale-switch tests |
| No event/raw-diagnostic shortcut | Existing repository checks reject known direct diagnostic serialization paths; behavioral tests cover non-obvious flows |

Static checks cannot prove that arbitrary strings are secret-free or that every
possible exception is reported. Document those limits. Private constructors,
typed adapters, constrained imports, negative fixtures, and integration tests
together provide the enforcement; regex checks alone are not the framework.

## 11. Compatibility and documentation

Adding localization and changing failed-turn nesting are explicit current-version
wire changes. Update schema, generated TS, Rust/TS SDKs, Desktop IPC, Web/TUI/CLI,
E2E fixtures, and examples together. Do not expose partial new contracts to users
or keep permanent aliases and runtime guessing between old/new error objects.

Failed-turn errors are persisted today. Use the existing upgrade mechanism for
an idempotent, transactional migration of affected stored payloads before strict
new decoding. Preserve codes, retry metadata, event identity/order, and available
public message text; apply reviewed disclosure rules to legacy diagnostic text.
Record any deliberate changes. Do not recapture or invent missing historical
stacks. Legacy decoding, if required, belongs only in that migration, not live
service/SDK fallback paths. Migration failure leaves the old data recoverable and
fails visibly before normal service begins. Rollback must account for the data
shape change; reverting binaries alone is not a safe rollback plan.

The confirmed database surface is `agent_events.payload_json`, owned by
`talos-store` session/agent_execution. Use the existing database migration lane
invoked by `talos-upgrade::run_startup_upgrades`, with `_sqlx_migrations` as its
completion ledger; do not register the same database change in Product Upgrade
state. Migration 202609060001 upgrades canonical failure payloads transactionally;
fixtures pin both shapes. Archived legacy timeline payloads are not read by the
current decoder. Event identities and ordering stay unchanged.
Migration 202609060002 upgrades embedded notice content and permits a null Agent
sequence for product notices in that same table. It preserves the sqlite_sequence
high-water even after event deletion and restores all Session cleanup targets.
Both migrations are part of coordinated protocol 0.4 deployment; raw event
consumers migrate to the tagged Agent/SystemNotice envelope and history notices.
The release procedure must require a verified, access-controlled, consistent
database backup before migration and restore it with the matching old binary for
rollback. Restoring the backup loses writes after its recovery point.
Disclosure sanitization can be irreversible; do not claim a reverse
JSON rewrite recovers the original text. Test interruption, restart, malformed
legacy records, and backup restoration before enabling strict new decoding.

Audit raw-event consumers, subagent summaries, recovery, CLI behavior, IPC, and
plugin/MCP status payloads as integration surfaces even if marked experimental.
Business behavior correction, such as preserving a budget code rather than
InternalError, is recorded as a behavior change for callers branching on codes.

The English error model is the single implementation reference; translated and
root legacy documents point to it. Root `AGENTS.md` records mandatory entry
points, ownership, and verification requirements. Execution stages and gate
results belong only in the separate plan.

## 12. Acceptance and abstraction review

Acceptance requires a known business failure and an unknown internal failure to
retain their intended code, specific public explanation, source diagnostics, and
captured stack through the real backend/frontend boundaries. A Desktop error
must be recognized as a typed application error after actual IPC. Failed-turn
replay must show the same public meaning without logging it again. Tests must
demonstrate that a broad translation cannot swallow a specific cause.

Measure failure-path stack capture and sink behavior without disabling required
diagnostics. Verify that successful requests do not pay stack-capture cost and
that cancellation/retry behavior does not change accidentally.

The retained abstractions each have a current owner and consumer: TalosError for
internal propagation, ApiError for shared transmission, Agent lifecycle kinds
for orchestration, tool results for execution feedback, and transport envelopes
for transport. Remove duplicate diagnostic strings, duplicated public DTOs,
lossy mappings, unconditional default-message reconstruction, and scattered UI
formatters. Add no error registry, global exception manager, generic details map,
error store, or backend localization engine.
