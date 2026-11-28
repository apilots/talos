# Execution-node Memory maintenance port

See the [distributed execution design](../design.md) and the current
[Worker execution ownership contract](../../refactor-046-worker-session-execution/execution-ownership.md).
The product owns scheduling, authorization, idle Session exclusion and idempotent
usage consumption. The target Worker owns model selection and credentials,
canonical execution history, formation policy, timezone and Workspace files.

## Call path

```text
Completed Turn → Worker built-in Memory reflection → local UserWorkspaceMemoryService
Dream scheduler/run-once → product idle reservation → managed dispatch readiness/activity
                        → bound AgentHarness.form_dream_memory
                        → Worker Config/router + KernelAgentProvider
                        → existing formation algorithm + local Memory files
                        → retained Worker formation fact → product Profile settlement/cursor
```

Before waking a managed Worker, the product qualifies known pending work from
retained source mappings and accepted commands. Missing or incomplete product
projections remain conservative candidates; an empty Session alone is not work.
The existing managed dispatch lifecycle coalesces automatic IdleSleep wake and
preserves manual-stop intent. The product revalidates scope and idleness after
readiness and retains the existing activity permit through usage settlement.
Global built-in job management retains its existing execution-Host ownership
boundary; owning an ordinary cloud Worker does not authorize Server-wide jobs.

Coordinator resolves the Session's bound Worker and checks ownership and
availability. Embedded and standalone Workers expose the same protected Harness
operation, SessionGate and configuration execution gate. Network requests carry
NodeAuth and coordinator identity. The target resolves Workspace scope using its
own configuration; product filesystem paths are not execution authority.

## Rust and gRPC

The [AgentHarness](../../../talos-rs/crates/orchestration/agent-protocol/src/port.rs)
Memory operations are:

| Method | Input | AgentResult output | gRPC |
| --- | --- | --- | --- |
| `read_workspace_memory` | `AgentSessionContext` | `UserWorkspaceMemorySnapshot` | `ReadWorkspaceMemory(SessionRequest)` |
| `update_workspace_memory` | `AgentSessionContext`, `UserWorkspaceMemoryUpdate` | `UserWorkspaceMemorySnapshot` | `UpdateWorkspaceMemory(UpdateWorkspaceMemoryRequest)` |
| `form_dream_memory` | `DreamMemoryParams` | `DreamMemoryResult` | `FormDreamMemory` |

`DreamMemoryParams` contains the authorized Session context and an optional
`DreamMemoryCursor { generation, offset }` naming the product's consumed position.
The result contains at most one `DreamMemoryFormation` and the next cursor. A
formation preserves source coverage, the complete Worker model usage with a
stable request ID, and an optional typed public failure. The product settles paid
usage even when formation fails; it then advances its own consumption cursor.
When no formation remains after that cursor, the result's `processed_until_seq`
snapshot lets the product reconcile already committed progress after an interrupted
formation without skipping any retained usage fact. Immediate authorization,
availability, binding and cursor failures retain the
unified `TalosError` contract.

The Worker first returns the next retained fact after the supplied cursor. Only
when no unconsumed fact exists does it attempt another bounded formation batch.
Observed model usage is durable before applying model output to Memory. An
interrupted invocation therefore retains its observed usage and explicit failure
for later consumption. The active protected reservation excludes another reader
until the invocation finishes or is canceled. Consumption does not acknowledge,
delete or mutate Worker facts; the existing Session generation clear/retirement
boundary owns their cleanup. Replaying a cursor does not call the model again.

The [protobuf definitions](../../../talos-rs/crates/orchestration/agent-pb/proto/harness.proto)
and [typed projection](../../../talos-rs/crates/orchestration/agent-protocol/src/dream_pb.rs)
carry only formation inputs/results. There is no Server model-prompt/reply
callback. Direct gRPC and registered transport preserve the same fields and
errors. The existing streaming RPC carries one request and one bounded result;
its total budget is 300 seconds and first-frame budget is 30 seconds. Transport
loss or lease revocation cancels the active operation; later product consumption
reads retained facts through the same operation and cursor.

Workspace Memory preserves Missing/template, Valid/document/version and
Invalid/raw-content/diagnostic states, six typed update operations, field
unchanged/set/clear semantics, and MustNotExist/Match revisions. The original
Markdown parser, validation and atomic file writes remain authoritative. File
operations retain their ten-second unary deadline and do not retry writes.

## Execution and settlement

Built-in Memory reflection runs from the Worker's bounded local completed-Turn
history using its active built-in model selection and provider router. Its
best-effort policy and `memory` usage settlement remain on the original Turn
completion path. Dream uses that same model-selection capability without owning
product Profile storage or depending on the product-only InnerModelClient.

Dream formation uses Worker Kernel policy and timezone, canonical history and
the existing oldest-uncovered Memory progress boundary. Successful local file
formation advances only the Worker's execution progress. Retained formation facts
are independent of product Timeline events and are never represented as a chat
Turn or Goal/Loop execution. Product Profile request-ID idempotency plus the
separate Dream consumption cursor prevent duplicate billing on retries. Product
view rebuild preserves both accounting and that cursor.

This port does not add cross-Worker migration, general model callbacks, a new
recovery service, an ACK/outbox protocol, or Session-free multi-node file browsing.
