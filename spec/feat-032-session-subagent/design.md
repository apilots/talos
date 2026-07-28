# Session Subagent

## 1. Decision

Talos implements Subagent as a thin typed control plane over the existing
Session, Turn, Agent, Timeline, and Tool runtime.

Every Subagent is a normal child Session with its own Turn history, model
context, inference loop, and Tool execution. The feature does not introduce a
second execution runtime, lifecycle state machine, event protocol, status
store, scheduler, registry, or client subscription system.

```text
Model
  -> six collaboration Tool handlers
  -> AgentControl
  -> SubagentService
  -> existing Session / Turn / Agent / Timeline owners
```

The tree identity is independent from execution identity:

```text
AgentPath -> logical address in one root tree
SessionId -> durable conversation identity
TurnId    -> one execution attempt
AgentId   -> exact persisted Agent definition
```

## 2. Goals

- Support `spawn_agent`, `send_message`, `followup_task`, `wait_agent`,
  `interrupt_agent`, and `list_agents`.
- Give every child a canonical recursive `AgentPath`.
- Preserve exact Agent binding for every child Turn.
- Authorize control operations by the caller's persisted tree membership.
- Reuse existing Turn start, input, interrupt, lifecycle, and Timeline facts.
- Keep model-facing arguments and results strongly typed and deterministic.
- Bound Tool inputs and results.
- Preserve Subagent Tool-card presentation in Web/Desktop and equivalent
  inline presentation in TUI.

## 3. Non-goals

- A new Agent runtime, Thread manager, lifecycle state machine, or task
  supervisor.
- A new Agent registry, graph store, event bus, status store, or transcript.
- Generic Timeline/ActiveTurn persistence redesign.
- Generic Storage transaction APIs.
- Generic Tool Result or Tool Handler framework redesign.
- Persistent/offline mailboxes or replayable input.
- Persistent or offline parent-message delivery.
- Startup orphan repair or fabricated terminal events.
- Direct HTTP/app-server Agent Tree query APIs.
- Client child-Timeline subscriptions or client-side Turn state reducers.
- Standalone Web/TUI Agent Tree navigation.
- Recoverable cross-resource delete markers.
- LRU residency, workflow scheduling, priority, distributed quota, or rollout
  budgets.

## 4. Ownership

### 4.1 Shared domain types

`talos-types` owns the one public set of Subagent domain types:

- `AgentPath`
- `AgentTarget`
- `AgentTurnRef`
- `SubagentRef`
- one arguments/result pair for each of the six Tools
- finite enums for operation outcomes and visible Subagent state

These types must not contain `serde_json::Value`, arbitrary maps, marker
strings, or ambiguous combinations of optional fields for core semantics.
Transport layers may add envelopes but may not copy the business objects.

### 4.2 SubagentService

`SubagentService` is the sole Subagent business owner. It:

- validates caller identity and request bounds;
- resolves canonical paths and exact targets;
- verifies root-tree membership and ancestry authorization;
- creates child Sessions through the existing Session owner;
- starts and continues Turns through the existing Turn owner;
- interrupts through the existing runtime interruption path;
- sends bounded messages through the existing exact-Turn Agent input path;
- reads authoritative Turn/ActiveTurn state and bounded Timeline facts.

It must not:

- own Agent execution tasks;
- retry or synthesize terminal persistence;
- scan all Sessions at startup to reconstruct lifecycle;
- infer lifecycle from an arbitrary Event window;
- maintain a second status record;
- access client presentation state;
- expose a generic framework for unrelated features.

### 4.3 AgentControl

`AgentControl` is the narrow Tool-facing port. It binds trusted caller
Session/Turn identity and delegates each typed operation one-to-one to
`SubagentService`.

It owns no Store access, semaphore, task, queue, lifecycle, projection, or
business validation.

### 4.4 Existing runtime owners

- Session service/store own durable Session records.
- Turn service owns Turn submission and lifecycle.
- Coordinator resolves the persisted exact `agent_id`.
- Agent runtime owns the live execution task and active input receiver.
- The Kernel runtime consumes running messages only at an existing model-round
  boundary and persists them as normal user input in the child Turn.
- Timeline owns durable execution facts and normal subscriptions.
- Tool infrastructure owns standard running/completed/error output.
- Rust SDK and TypeScript SDK transport the same server-owned typed
  presentation objects.
- Clients only map SDK objects into the existing Tool presentation surfaces.

Subagent code must not duplicate these responsibilities.

The authorization matrix is fixed:

- root may target any strict descendant in its own tree;
- a child may target only its own strict descendants;
- self, ancestors, siblings, and nodes in another root are rejected;
- a relative target names one direct child;
- a canonical path may name any strict descendant authorized above.

## 5. Durable identity

The minimum child Session identity is:

```text
parent_session_id
parent_turn_id
tree_root_session_id
agent_path
agent_id
```

Rules:

- Primary Session path is `/root`.
- A direct child path is `parent_path + "/" + segment`.
- A canonical path is unique within one `tree_root_session_id`.
- `agent_id` is resolved at spawn and persisted; later Turns use that exact
  value rather than rerouting by mutable configuration.
- Tree identity cannot be mutated through ordinary Session patch APIs.
- Parent/root/path relationship is validated by one application owner.
- Persistence enforces necessary nullability and uniqueness, but does not
  duplicate the full business rule in multiple triggers and services.
- Store-wide query defaults retain their existing semantics. Product Session
  lists explicitly filter Primary Sessions when required.

No separate registry or graph persistence is added.

## 6. Execution

### 6.1 Spawn

```text
spawn_agent
  -> validate caller and child segment
  -> authorize caller in root tree
  -> resolve exact Agent definition
  -> create child Session identity
  -> submit child Turn through existing TurnService
  -> return child path/session/turn reference
```

If Turn submission fails before acceptance, spawn fails. Subagent must not
translate a missing first event into success or manufacture a replacement
terminal event.

Subagent does not add its own semaphore or pass a feature-specific lease
through generic Turn execution. Concurrency remains owned by the existing
Agent/Turn runtime. A root-tree limit is deferred until that owner provides a
lifecycle-bound admission API.

### 6.2 Follow-up

`followup_task` appends a new user task using the existing Turn submission
path. It is valid only when the target has no active Turn. It preserves the
child Session's exact persisted `agent_id`.

### 6.3 Wait

`wait_agent` waits for authoritative existing Turn/Timeline activity and
returns a bounded typed snapshot. It is not `join(thread)` and does not create
a new subscription protocol.

The Tool handler performs one `AgentControl::wait` call. Target resolution,
current Turn selection, waiting, and final snapshot projection stay within
`SubagentService`; no separate inspect API is exposed to the Tool layer.

The implementation may use existing Timeline wake-up/replay APIs internally,
but it must not expose child subscriptions to clients or reconstruct a second
Turn state machine.

Running snapshots use a bounded exact-Turn Event tail. Completed output uses
the persisted latest exact-Turn artifact through the existing history owner,
using the shared message ID to verify its identity because execution Turn IDs
and artifact Turn IDs belong to different owners. It applies explicit output
byte limits and never scans the full Session
history or treats a truncated Event tail as a complete terminal transcript.

After process restart, Subagent inherits the existing Turn/ActiveTurn owner's
restart semantics without adding a residency probe or a second interpretation
of persisted state. It does not scan Sessions, synthesize failure, or infer
residency. Clients render the state returned by the existing owner.

### 6.4 Interrupt

`interrupt_agent` resolves the exact active Turn and delegates to the existing
interrupt operation. Existing runtime state remains authoritative.

### 6.5 List

`list_agents` returns a bounded typed view of nodes authorized for the caller.
It is a model Tool, not a direct Session product endpoint.

List state comes from authoritative Session/Turn/ActiveTurn data. Timeline may
provide bounded display facts, but absence of an event inside a retention
window must not be interpreted as a new lifecycle state.

## 7. Tool surface

Each Tool has one handler:

```text
decode typed arguments
  -> obtain trusted caller context
  -> call one AgentControl method
  -> encode typed result with existing Tool output
```

Handlers do not access Stores or runtime maps, and they do not contain
lifecycle policy.

Successful results use the existing Tool output path. Errors use the existing
Tool error path with contextual typed service errors. This feature does not
change the generic `ToolResultEnvelope`, output persistence policy, preview
policy, or model payload logic.

`wait_agent` may remain running while the service waits, using the standard
Tool running state. No `CollaborationToolProgress` field is added to generic
Timeline deltas. This iteration carries detailed Subagent presentation data
only in the final canonical Tool result.

## 8. SDK and client presentation

The Rust service is the only owner of Subagent state and presentation data.
The existing Tool result envelope carries one
`TalosContentItem::SubagentResult { result: CollaborationToolResult }` leaf;
operation results contain `SubagentSnapshot` where applicable. The result is
never downgraded to `JsonContentValue` or `unknown`.

Rust SDK re-exports the canonical Rust types to TUI. TypeScript SDK exposes
generated types from the same schema to Desktop/Web. They may decode the
normal protocol envelope once, but they must not reconstruct lifecycle, merge
child Event streams, or define an independent business DTO. Generated wire
types are required; repeated hand-written validators are forbidden.

Desktop/Web keep a Subagent Tool card, and TUI keeps an equivalent inline Tool
projection. They display only SDK-provided fields:

- operation and authoritative running/completed/error state;
- canonical path, Agent identity, parent/child relation, and Turn reference;
- spawn, follow-up, wait, interrupt, and list outcomes;
- terminal output, latest bounded snapshot, and hierarchy supplied by the
  service.

While a Tool is executing, clients display only the existing standard Tool
running state. There is no additional live Subagent progress contract in this
iteration.

Clients must not:

- subscribe recursively to child Sessions;
- derive state from Event windows or optional-field combinations;
- treat subscription/network/display failures as Agent execution failures;
- query database or internal runtime state;
- maintain separate Rust/TS/Web/TUI semantic parsers;
- add standalone Agent Tree navigation or a local command that bypasses the
  model Tool.

If the existing parent Tool stream cannot carry a display fact without a
generic Timeline redesign, that fact is omitted from this iteration.

## 9. Deletion

Normal Session deletion remains owned by the existing Session boundary. When
deleting a Primary with Subagents, that owner rejects active descendants and
removes the inactive durable descendant tree with one database delete
statement. A Session delete trigger cleans dependent Turn/Event/idempotency
rows inside the same atomic statement.
This feature does not add a generic transaction coordinator, delete journal,
staging directory, or recovery daemon.

## 10. Bounds and errors

- Canonical path depth, segment length, response node count, input size, and
  result size have explicit constants.
- Target inactive, unauthorized target, invalid path, duplicate child, and
  execution failure are distinct typed errors.
- No fallback changes an error into success.
- Diagnostic strings are for humans; callers do not parse them.

## 11. Compatibility

This iteration intentionally changes only:

- child Session persistence fields and necessary uniqueness;
- exact Agent dispatch for persisted child Sessions;
- Tool registration replacing the legacy one-shot Agent Tool;
- internal application wiring for `SubagentService` and `AgentControl`;
- canonical Subagent type exposure in Rust SDK/TypeScript SDK and bounded
  adaptation in existing Web/Desktop/TUI Tool presenters.

It intentionally does not change generic Session query defaults, Timeline
wire events, Tool Result policy, app-server methods, HTTP routes, or client
execution state machines. SDKs and existing Tool views add only the canonical
Subagent typed projection.

## 12. Verification

Required scoped verification:

- canonical path and persistence constraints;
- ancestry authorization and exact target resolution;
- exact persisted Agent routing;
- spawn acceptance and failure propagation;
- follow-up only after inactivity;
- interrupt delegation;
- bounded wait/list results;
- five Tool argument/result and caller-context mappings;
- absence of direct Agent Tree APIs and collaboration-specific Timeline fields.

The final review must inspect both missing behavior and excess behavior. A
passing test does not justify infrastructure outside these boundaries.
