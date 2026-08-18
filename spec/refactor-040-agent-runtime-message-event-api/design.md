# Agent Runtime Message, Event, and Active Run API Standardization

Status: draft
Owner: agent
Created: 2026-08-11
Updated: 2026-08-17
Related: `feat-032-session-subagent`, `feat-033-user-file-storage-model-input`, `feat-037-tool-permission-user-interaction`, `refactor-038-llm-model-selection`

## 1. Decision Summary

Talos keeps the existing `AgentRuntime` abstraction and standardizes the Agent layer around five contracts:

```text
AgentMessage
AgentInput / AgentOutput
AgentEvent
AgentRuntime
ActiveRunControl
```

The target execution path remains:

```text
Gateway / App Server / SDK
    -> Talos Service
        -> Agent Coordinator
            -> Agent Harness
                -> AgentRuntime implementation
                    -> Agent Kernel
                        -> LLM Provider
```

The main decisions are:

1. `AgentMessage` is the only conversation object at the Agent and upper boundaries.
2. `AgentEvent` is the only Agent-level live and replay event contract.
3. `AgentRuntime` remains the only Harness-to-backend execution SPI.
4. Agent Harness owns live Run lifecycle and one small `ActiveRunControl` per executing Runtime invocation.
5. Talos Service owns durable Turn state, UserAction state, Timeline persistence, recovery, and multi-client access.
6. `interrupt` cancels the complete logical Run and all locally owned or externally cancellable Run-scoped operations. It is not a single-Tool cancellation API.
7. Multi-client `interrupt_turn` is asynchronous: acceptance returns quickly, while terminal completion is observed through Timeline subscription or recovery.
8. Cooperative cancellation is bounded by a grace timeout and followed by force abort. No cancellation path may wait without a hard deadline.
9. Tool implementations own cleanup of the tasks, streams, processes, and requests they start. Detached Run-scoped work is forbidden.
10. No `TurnExecutionManager`, `ExecutionTaskManager`, `ExecutionResourceRegistry`, parallel event bus, or new Session lifecycle framework is introduced.

## 2. Current Problems

Talos already has the intended layers, but the current contracts and lifecycle behavior are fragmented:

1. Initial input, active-run input, Runtime input, Timeline input, and history use overlapping objects.
2. Kernel output is repeatedly copied into Runtime, Coordinator, Service, history, and client DTOs.
3. Kernel exposes internal loop, processor, scheduler, persistence, and Timeline implementation types as public API.
4. Runtime-family names are mixed with Agent identity and unsupported Runtime placeholders.
5. Harness tracks active Runs with an abort-only handle. Interrupt drops the main Runtime task without first propagating cooperative cancellation.
6. Tool and Bash runtimes can start detached tasks or processes that outlive the aborted Runtime future.
7. `WaitingUserAction` is durable product state, but interrupt currently assumes a live Runtime handle.
8. `ActiveTurnStatus::Cancelling` exists, but normal event reduction can overwrite it with `Running`.
9. Clients treat interrupt acknowledgement as terminal completion and can display idle while backend cleanup is still running.
10. Existing tests prove that an abort method was called, not that Run-scoped work actually stopped and durable state reached a terminal result.

The design must solve the API duplication and the lifecycle gap without creating a generic task platform.

## 3. Goals

1. Establish one Agent input, output, message, event, error, and lifecycle API.
2. Keep Agent Harness independent from Talos Service, stores, Gateway, and concrete Runtime implementations.
3. Keep Agent Kernel and LLM Provider independently usable lower libraries.
4. Make unary output, streamed completion, Timeline replay, history, SDK, Web, Desktop, and TUI converge on the same message semantics.
5. Make interruption a stable Agent API with bounded internal cleanup and asynchronous product semantics.
6. Ensure a Turn cannot reach terminal state while confirmed Run-scoped work is still active.
7. Preserve waiting-input continuation, files, tools, compaction, Subagent, title, usage, replay, and recovery behavior.
8. Delete duplicate DTOs, field-copy mappings, placeholder Runtime variants, obsolete public exports, and legacy client business unions.
9. Enforce logical ownership through the physical Cargo dependency graph.

## 4. Non-Goals

1. Do not introduce an `Engine` abstraction or rename Runtime to Engine.
2. Do not add a generic Runtime, task, process, resource, or plugin registry.
3. Do not implement Codex or Claude Code Runtime support before a real implementation exists.
4. Do not add a public `cancel_tool(call_id)` API in this refactor.
5. Do not add MCP protocol-level cancellation before the current RMCP capability and behavior are independently specified.
6. Do not add an `ExecutionUnknown` public Run or Tool status. Unconfirmed cleanup uses an existing failed terminal state with a typed error and a separate Session execution gate.
7. Do not move Timeline, ActiveTurn, UserAction, Session, or idempotency persistence into Agent Harness.
8. Do not expose backend thread IDs, process IDs, store records, raw Provider events, or Kernel-private state.
9. Do not preserve removed APIs through aliases, dual serialization, fallback readers, or parallel event paths.
10. Do not redesign Agent definition CRUD, project CRUD, file CRUD, authentication, or plugin administration.

## 5. Terminology and Ownership

| Concept | Owner | Meaning |
| --- | --- | --- |
| Session | Talos Service | Durable product conversation and the only public `session_id` source |
| Turn | Talos Service | Durable product unit started by one user request |
| Run | Agent Harness | Agent execution for the Turn; `run_id` is the product `turn_id` |
| Runtime invocation | Agent Runtime | One active backend execution segment; a waiting continuation may create another invocation for the same Run |
| Operation | Runtime / Tool | One model request, Tool call, Bash process, MCP request, or Subagent execution |
| UserAction | Talos Service | Durable request waiting for user input; not an executing task |
| ActiveRunControl | Agent Harness | In-process control for the currently executing invocation of one logical Run, including cancellation, hard abort, and cleanup |
| ActiveTurnRecord | ActiveTurnStore | Durable product state visible to recovery and clients |
| AgentEvent | Agent Harness API | Transport-neutral Run and message lifecycle event |
| TimelineEventEnvelope | Talos Service | Persisted Agent event plus product identity and recovery metadata |

One product Turn maps to one logical Agent Run. A Run may contain more than one Runtime invocation when it waits for input and later continues.

The Run control identity is always the pair:

```text
(conversation_id, run_id) == (session_id, turn_id)
```

Lookup may be indexed by `session_id` because only one Turn can be active per Session, but every mutation and interrupt must also compare `turn_id`. A stale client must never cancel a newer Turn.

## 6. Layer Boundaries

### 6.1 Responsibility Flow

```text
Gateway / App Server
  - transport decoding, authentication envelope, schema

Talos Service
  - product validation
  - durable Session, Turn, UserAction, Timeline, and recovery state
  - multi-client command and subscription APIs

Agent Coordinator
  - Agent selection, binding, routing, and forwarding

Agent Harness
  - Agent API normalization
  - one active Run per Session
  - live input, interrupt, timeout, completion, and terminal events

AgentRuntime
  - backend adapter and one Runtime invocation

Agent Kernel
  - model/tool loop, prompt, memory, compaction, and Tool scheduling

LLM Provider
  - provider-neutral model request and normalized model stream
```

### 6.2 Physical Packages

| Package | Owns | Must not depend on |
| --- | --- | --- |
| `talos-common` | Stable dependency-light values such as model selection and token usage | config, Agent, Kernel, Service, store, Gateway |
| `talos-agent-api` | Agent message, input, output, event, Harness SPI, Runtime SPI, lifecycle params, errors | config, Kernel, Provider, Service, store, Gateway |
| `talos-agent` | Generic Harness implementation and `ActiveRunControl` | config, Kernel, Provider, Service, store, Gateway |
| `talos-agent-runtime-kernel` | Agent-to-Kernel projection and Kernel Runtime assembly | Coordinator, Service, Gateway |
| `talos-agent-kernel` | Kernel facade and Kernel-owned contracts | Agent API, Agent Harness, Service, Gateway, product stores |
| `talos-coordinator` | Agent registry, routing, binding, forwarding | concrete Runtime, Kernel, Provider, Service, store |
| `talos-service` | Product orchestration, persistence, recovery, history | concrete Runtime and Kernel execution internals |
| Gateway / App Server | Protocol methods and generated schemas | Runtime, Kernel, store records |

`talos-common` is a leaf reuse boundary, not a catch-all. Lifecycle, persistence, transport, and domain-specific objects stay with their real owner.

## 7. Cross-Layer Object Rules

Every boundary relationship uses exactly one of these forms:

1. **Direct reuse** when semantics and ownership are unchanged.
2. **Composition** when an upper layer adds routing, persistence, or transport metadata.
3. **Semantic projection** when the receiving layer has a genuinely different abstraction.
4. **Boundary stop** when an object is private to its owner.

Examples:

```text
Coordinator forwards AgentRunParams unchanged.
Coordinator wraps AgentOutput with AgentSelection.
Kernel Runtime projects AgentKernelOutput into AgentOutput.
Service wraps AgentEvent in TimelineEventEnvelope.
Provider raw SSE stops at the Provider adapter.
Runtime abort handles stop at Agent Harness.
Store records stop at Service.
```

Field-for-field mappings into another layer-prefixed DTO are forbidden.

## 8. Canonical Agent Message Model

`AgentMessage` is the stable conversation object used by Agent Harness, Runtime, history, SDK, and clients:

```rust
pub struct AgentMessage {
    pub message_id: String,
    pub role: AgentMessageRole,
    pub blocks: Vec<AgentMessageBlock>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

pub struct AgentMessageBlock {
    pub block_id: String,
    pub parent_block_id: Option<String>,
    pub status: AgentMessageBlockStatus,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub content: AgentMessageBlockContent,
}

pub enum AgentMessageBlockContent {
    UserInput(AgentUserMessage),
    Reasoning { text: String },
    AssistantText { phase: AgentAssistantPhase, text: String },
    ToolCall { call: AgentToolCall },
    ToolResult { result: AgentToolResult },
    ExecutionSummary { text: String },
    ContextCompaction { compaction: AgentCompactionMessage },
    SystemNotice { notice: AgentSystemNotice },
    Collaboration { collaboration: AgentCollaborationMessage },
}
```

The model preserves:

- ordered text, file, image, voice, and video input;
- reasoning, commentary, unknown, and final-answer phases;
- typed Tool arguments, progress, rich result content, errors, and call correlation;
- queued, running, waiting, completed, failed, and cancelled block states;
- compaction, system notice, and collaboration content;
- stable `message_id`, `block_id`, `call_id`, and parent identity.

Tool results are Tool-role messages correlated by `call_id`; they are not embedded as mutable result fields inside Tool-call blocks. Session history groups unchanged messages by Turn instead of inventing a second history-message object.

Skill selection, model selection, workspace, policy, and trusted execution context are not message content. They belong to `AgentInput.options` or `AgentRunParams`.

## 9. Agent Input, Output, and Events

```rust
pub struct AgentInput {
    pub message: AgentMessage,
    pub options: AgentInputOptions,
}

pub struct AgentRunContext {
    pub run_id: String,
    pub conversation_id: String,
    pub principal: Option<AgentPrincipal>,
    pub workspace: Option<AgentWorkspaceContext>,
    pub environment: Option<AgentEnvironment>,
}

pub struct AgentRunParams {
    pub context: AgentRunContext,
    pub input: AgentInput,
    pub policy: AgentRunPolicy,
    pub continuation: Option<AgentContinuation>,
}

pub struct AgentOutput {
    pub messages: Vec<AgentMessage>,
    pub stop_reason: AgentStopReason,
    pub metrics: AgentRunMetrics,
    pub model: Option<ModelRef>,
}

pub struct AgentRunFailure {
    pub error: AgentErrorInfo,
    pub messages: Vec<AgentMessage>,
    pub metrics: AgentRunMetrics,
    pub model: Option<ModelRef>,
}

pub enum AgentRunResult {
    Completed(AgentOutput),
    Failed(AgentRunFailure),
}
```

`AgentOutput.messages` contains newly materialized messages in deterministic order. Parallel `output_text`, `output_items`, and `reasoning` fields are removed. `model` is present when model selection completed; cancellation or failure before selection may leave it absent. `AgentRunFailure` preserves the same bounded partial messages and metrics so failure, history, and recovery do not discard already visible output.

The outer and inner failure boundaries are strict. `AgentResult::Err` is allowed only before Harness creates the live Run and emits `RunStarted`, for example validation, identity conflict, or capability rejection. After `RunStarted`, every Runtime, Kernel, Provider, Tool, cancellation, and cleanup error is materialized as `AgentRunResult::Failed` and the Run emits exactly one `RunFailed`. If Runtime SPI returns an outer error after start, Harness converts it with the private per-Run reducer's bounded partial messages, latest metrics, and selected model; it never leaks that error as a second unary-only outcome.

The stable Agent event set is:

```rust
pub enum AgentEvent {
    RunStarted(AgentRunStarted),
    MessageStarted(AgentMessageStarted),
    MessageBlockStarted(AgentMessageBlockStarted),
    MessageBlockDelta(AgentMessageBlockDelta),
    MessageBlockCompleted(AgentMessageBlockCompleted),
    MessageCompleted(AgentMessage),
    WaitingForInput(AgentPendingInput),
    RunMetrics(AgentRunMetrics),
    RunCompleted(AgentOutput),
    RunFailed(AgentRunFailure),
}
```

Rules:

1. Harness alone emits `RunStarted`, `WaitingForInput`, and terminal events for live invocations. Service may materialize the single terminal event only for a durably waiting or orphaned Run with no live `ActiveRunControl`.
2. Runtime emits message and metrics events through a private `AgentRuntimeEventSink` that rejects Run boundary variants.
3. Every stable Run emits exactly one final `RunCompleted` or `RunFailed` event.
4. `RunMetrics` is live-only; final metrics are carried by `AgentOutput`.
5. `MessageCompleted(message)` and the same-ID terminal output message are equal.
6. Reducers upsert by `(message_id, block_id)` and never append terminal duplicates.
7. Cancellation is normally `RunCompleted` with `AgentStopReason::Cancelled`.
8. A cancellation whose hard-stop cleanup cannot be confirmed is `RunFailed` with a typed `cancellation_unconfirmed` error.

## 10. Agent Harness and Runtime APIs

Agent Harness exposes the minimum control surface:

```text
run(AgentRunParams) -> AgentResult<AgentRunResult>
run_streamed(AgentRunParams) -> AgentResult<AgentRunStream>
submit_input(AgentSubmitInputParams) -> AgentSubmitInputResult
interrupt(AgentInterruptParams) -> AgentInterruptResult
```

For streamed execution, a pre-start rejection returns `AgentResult::Err` without creating a stream or emitting `RunStarted`. After `Ok(stream)` is returned, execution errors settle through the stream's single `RunFailed` terminal event and completion result; they do not reappear as an outer error.

`AgentInterruptParams` contains stable identity only:

```rust
pub struct AgentInterruptParams {
    pub conversation_id: String,
    pub run_id: String,
    pub reason: AgentInterruptReason,
}

pub struct AgentInterruptResult {
    pub disposition: AgentInterruptDisposition,
}

pub enum AgentInterruptDisposition {
    Accepted,
    AlreadyCancelling,
}
```

`interrupt` is an asynchronous control command. `Accepted` means the cancellation signal and bounded cleanup process were accepted. It does not mean the Run is already terminal.
Terminal idempotency belongs to Service because Harness removes terminal controls; a missing live Run is not retained in a second in-memory history map.

The Runtime SPI remains one execution method:

```rust
/// Adapts one backend implementation to the shared Agent execution contract.
pub trait AgentRuntime: Send + Sync {
    fn kind(&self) -> AgentRuntimeKind;

    fn capabilities(&self) -> AgentCapabilities;

    fn run(
        &self,
        params: AgentRunParams,
        io: AgentRuntimeIo,
    ) -> impl std::future::Future<Output = AgentResult<AgentRuntimeCompletion>> + Send;
}

pub struct AgentRuntimeIo {
    input_rx: Receiver<AgentInput>,
    events: AgentRuntimeEventSink,
    cancellation: AgentCancellation,
    supervisors: AgentRunSupervisorSpawner,
}

pub enum AgentRuntimeCompletion {
    Finished {
        result: AgentRunResult,
        checkpoint: Option<AgentPersistenceCheckpoint>,
    },
    WaitingForInput {
        pending: AgentPendingInput,
        checkpoint: AgentPersistenceCheckpoint,
    },
}
```

Runtime returns a final `AgentRunResult` or a waiting completion. `WaitingForInput` is returned only after the invocation has stopped starting operations and every registered supervisor has confirmed that no Run-scoped work remains. Harness then closes the invocation event sink, removes `ActiveRunControl`, and emits `WaitingForInput`. Runtime-private thread IDs, SDK sessions, abort handles, tasks, and continuation tokens do not cross the Harness boundary.

`AgentRunSupervisorSpawner` is a narrow, non-enumerable registration handle defined in `talos-agent-api`:

```rust
/// Registers resource-owning supervisors in the current Run task group.
pub struct AgentRunSupervisorSpawner {
    inner: Arc<dyn AgentRunSupervisorRegistration>,
}

/// Identifies one bounded supervisor entry for diagnostics and cleanup.
pub struct AgentRunSupervisorParams {
    pub operation_id: String,
    pub operation_kind: AgentRunOperationKind,
}

/// Classifies the owner boundary of a supervised Run operation.
pub enum AgentRunOperationKind {
    Model,
    Tool,
    Mcp,
    Subagent,
}

/// Creates a supervisor only after Harness has reserved its task-group entry.
pub type AgentRunSupervisorFactory = Box<
    dyn FnOnce(AgentCancellation) -> AgentRunSupervisorFuture + Send,
>;

/// Owns one operation until owner-specific cleanup reaches a bounded result.
pub type AgentRunSupervisorFuture = Pin<
    Box<dyn Future<Output = AgentOperationCleanupResult> + Send + 'static>,
>;

/// Explains why a supervisor factory could not be accepted.
pub enum AgentRunSupervisorRegistrationError {
    Closed,
    CapacityExceeded,
}

/// Accepts a supervisor factory without exposing Harness task-group state.
pub trait AgentRunSupervisorRegistration: Send + Sync {
    fn spawn(
        &self,
        params: AgentRunSupervisorParams,
        factory: AgentRunSupervisorFactory,
    ) -> Result<(), AgentRunSupervisorRegistrationError>;
}
```

The concrete boxed future aliases remain dependency-light Agent API types. `spawn` first reserves an entry in the bounded Harness task group and only then invokes the factory; the factory must create the resource inside its returned supervisor future. If registration is closed or capacity is exhausted, `spawn` returns a typed error and the factory is never invoked, so no unowned resource can start.

Kernel does not import Agent API. Kernel Runtime projects this handle into a Kernel-owned registration port, and Kernel projects the same capability into the existing Tool execution context. These are dependency-boundary facades backed by the same Harness registration entry, not additional task groups or registries. No layer can enumerate, address, or cancel another operation through this port.

Kernel is the only current `AgentRuntimeKind` and `AgentRuntimeConfig` variant. Codex and Claude Code remain architecture examples until implemented and tested.

## 11. Active Run Control

### 11.1 One Small Harness-Owned Abstraction

Harness reuses its existing `active_turns` map and replaces the abort-only `ActiveTurnHandle::Running` state with one `ActiveRunControl`:

```rust
/// Controls one logical Agent Run without owning product persistence.
pub(crate) struct ActiveRunControl {
    identity: AgentRunIdentity,
    cancellation: AgentCancellation,
    tasks: RunTaskGroup,
    phase: ActiveRunPhase,
    invocation_generation: u64,
    runtime_abort: Option<AbortHandle>,
    input_tx: Option<Sender<AgentInput>>,
    completion: RunCompletionSignal,
}

pub(crate) enum ActiveRunPhase {
    Reserved,
    Running,
    Cancelling,
    Terminal,
}
```

The exact synchronization primitives are implementation details. The required semantics are:

- identity check uses both conversation and Run identity;
- cancellation is idempotent and one-way;
- Runtime abort and input handles remain attached while their invocation is live, including during the transition to `Cancelling`;
- the cleanup supervisor exists from Run start, not from the first interrupt call;
- completion is observable without polling arbitrary tasks;
- only one Runtime invocation can be active for the Run;
- every Runtime invocation receives a monotonic generation and a generation-bound event sink;
- resumed generations come from the validated durable continuation, not an in-memory waiting control;
- the control is removed after final terminal delivery, or before Harness emits `WaitingForInput` for a cleanly settled waiting invocation.

No resource registry or separately addressable task-management API is part of this abstraction.

`RunTaskGroup` is the Harness-private implementation behind `AgentRunSupervisorRegistration`, not a general task manager. It accepts only Run-scoped supervisor futures, closes registration when cancellation starts, and provides one bounded cleanup report:

```rust
pub(crate) enum RunCleanupReport {
    Confirmed,
    Unconfirmed { operations: Vec<OperationCleanupFailure> },
}
```

Each asynchronous operation registers one supervisor that owns its resources and child joins. Registration succeeds before the supervisor factory may start the resource. The supervisor remains owned by `RunTaskGroup` if the main Runtime future is aborted, so Harness can wait for local cleanup confirmation without understanding the resource type. The failure list is bounded and diagnostic-only.
At the final deadline, the group records every unfinished supervisor as unconfirmed, aborts the remaining supervisor tasks, and settles its report; no Tokio task handle remains attached to the Run. Resource drop guards still initiate their last owner-specific hard stop. Supported local resources such as Bash must normally confirm termination before this deadline and are covered by integration tests.

### 11.2 Structured Execution Tree

Every active operation derives cancellation from the Run root:

```text
ActiveRunControl
  -> Runtime invocation
      -> model request
      -> Tool call
          -> Tool-local reader / waiter / timeout / process
      -> MCP request
      -> Subagent operation
```

Cancelling the root reaches every active child. Cancelling one child operation, when a future Tool-specific API exists, must not cancel its parent Run. This design creates child cancellation scopes now but does not add a public operation index or targeted Tool API without a current caller.

### 11.3 Interrupt Flow

The complete asynchronous product and execution flow is:

```text
Client A interrupt_turn(session_id, turn_id)
    -> Service transaction: ActiveTurn -> Cancelling, cancel UserAction, append status event
    -> Service calls Harness interrupt with the exact Run identity
    -> Harness marks ActiveRunControl Cancelling and signals the Run root
    <- Service returns Cancelling without waiting for cleanup

ActiveRunControl cleanup supervisor
    -> Runtime invocation
        -> model / Tool / MCP / Subagent child scopes
        -> registered resource supervisors
    -> grace deadline
    -> optional force phase and Runtime abort
    -> bounded cleanup report
    -> exactly one RunCompleted(Cancelled) or RunFailed(...)
    -> Service terminal transaction
        -> close Timeline items and ActiveTurn
        -> keep Session gate Open when cleanup is confirmed
        -> set CleanupUnconfirmed when residual work is possible
    -> publish committed events to Client A and every other subscriber
    -> dispatch the next product Turn only when the Session gate is Open
```

```text
Service persists Cancelling
    -> Coordinator forwards stable identity
        -> Harness validates ActiveRunControl identity
            -> mark control Cancelling
            -> trigger root cancellation
            -> return Accepted immediately

cleanup supervisor
    -> close RunTaskGroup to new supervisors
    -> wait for cooperative Runtime completion until grace deadline
    -> if deadline expires, signal force phase and abort Runtime task
    -> RunTaskGroup supervisors continue Tool-owned hard stop
    -> wait for Runtime settlement and RunCleanupReport until the final bounded deadline
    -> close open blocks and Tool items
    -> emit RunCompleted(Cancelled) after confirmed cooperative cleanup,
       RunFailed(forced_abort) after confirmed force cleanup,
       or RunFailed(cancellation_unconfirmed) after unconfirmed cleanup
    -> settle completion and remove ActiveRunControl
```

Repeated interrupt calls return `AlreadyCancelling`. A missing live control returns `RunNotFound`; Service resolves idempotency from durable state and performs orphan recovery only when that state says the Turn was Running. An identity mismatch returns `RunNotFound` or `RunConflict`; it never cancels the currently active different Run.

### 11.4 Timeout Semantics

Cancellation uses two bounded periods:

1. **Grace period**: Runtime and Tools cooperatively stop and confirm cleanup.
2. **Force-settlement period**: Harness aborts the Runtime task while RunTaskGroup supervisors continue hard stop and report bounded cleanup completion.

Both durations come from one resolved Harness lifecycle configuration. They are not per-client options and are not exposed as untrusted request fields.

`AgentCancellation` exposes one-way requested and force phases. Runtime and Tool code normally reacts to requested cancellation; resource supervisors also react to force cancellation and execute their owner-specific hard stop. A child scope inherits both phases, while cancelling a child does not cancel its parent.

No loop may wait indefinitely for an active task, process, channel, stream, or external request. Entering force phase and cleanup confirmation are independent facts. A forced Runtime abort with confirmed operation cleanup fails as `forced_abort`; any unconfirmed operation cleanup fails as `cancellation_unconfirmed`, which takes precedence and carries the bounded operation diagnostics.

### 11.5 Synchronous and Asynchronous Work

All Run-scoped work is represented by an async parent future even when it calls synchronous code.

- Non-blocking deterministic transformations may execute inline.
- Long synchronous work must cooperatively observe cancellation or run in an independently terminable process.
- `spawn_blocking().abort()` is not a valid stop guarantee because an already running closure continues executing.
- Run-scoped `tokio::spawn` is allowed only inside a structured owner that joins or aborts it before the parent operation completes.
- Detached tasks that can outlive the Run are forbidden.

Application-level resources such as an MCP connection pool remain alive. Only the Run-scoped request is cancelled.

## 12. Tool Cleanup Contract

Cancellation signalling is not cleanup completion. A Tool future may return `Cancelled` only after the resources it started are stopped or explicitly reported as unconfirmed.

The cancellation signal and restricted Run-scoped supervisor spawner propagate through the existing execution path:

```text
AgentRuntimeIo
  -> Kernel Runtime projection
      -> Kernel-owned registration port
          -> Agent Kernel execution context
              -> ToolExecutionContext
                  -> ToolContext
```

Each Tool owns its cleanup strategy. Harness does not understand Bash processes, MCP requests, file descriptors, or provider streams. A resource-owning Tool registers one supervisor through the Run-scoped task spawner before starting the resource. Tool-local channels may be prepared first, but resource creation occurs only inside the accepted supervisor factory. The supervisor owns the resource, its child tasks, hard-stop behavior, and cleanup confirmation; the Tool future communicates with it instead of owning detached resource tasks.

Cancellation completion is strongly typed internally:

```rust
/// Reports whether a supervised operation confirmed owner-specific cleanup.
pub enum AgentOperationCleanupResult {
    Confirmed,
    Unconfirmed { reason: AgentErrorInfo },
}
```

Runtime and Harness aggregate these results into `RunCleanupReport`. A Tool cannot return an ordinary cancelled result while reporting unconfirmed cleanup; the latter always projects to `RunFailed(cancellation_unconfirmed)`.

### 12.1 Bash

Bash is the first required structured-cleanup implementation:

```text
Run cancellation
  -> Bash supervisor stops new progress registration
  -> terminate process group
  -> wait grace period
  -> kill process group if still running
  -> join stdout reader, stderr reader, waiter, and timeout task
  -> remove Bash session
  -> return Cancelled
```

The Bash supervisor owns its `RunningToolProcess` and local tasks. Current detached reader, waiter, and timeout tasks must be replaced by a locally joined structure. Process ID and Bash runtime session ID remain private.

### 12.2 Model, MCP, and Subagent Operations

- Model streams stop local consumption on cancellation and close their operation future. Server-side inference that has no cancellation acknowledgement is not treated as a locally active task after the response channel is closed.
- MCP request futures stop local waiting and use a bounded deadline. A side-effecting remote operation without cancellation acknowledgement produces `cancellation_unconfirmed`; the shared MCP connection is not closed with the Run.
- Subagents started by the Run receive a child cancellation scope and must settle their bounded result state.
- Filesystem and search subprocesses follow the same process ownership rule as Bash when they start external processes.

Protocol-level remote cancellation is added only when the concrete backend supports it and contract tests prove the behavior.
An arbitrary remote Tool is treated as side-effecting unless a Talos-owned adapter or policy marks it read-only from trusted configuration. Untrusted MCP annotations alone cannot downgrade this conservative classification.

## 13. Waiting UserAction Is Not a Running Task

`WaitingUserAction` is durable Service state. It has no live Runtime invocation, `ActiveRunControl`, cancellation token, abort handle, or registered supervisor.

Runtime returns `AgentRuntimeCompletion::WaitingForInput` only after all Run-scoped work for the invocation is confirmed stopped. Harness closes the generation-bound event sink, removes `ActiveRunControl`, and only then emits `WaitingForInput`. Service persists the pending action and `WaitingUserAction` from that event.

The semantics are distinct:

```text
UserAction Deny
  -> persist rejection
  -> resume the same Run with the durable continuation

UserAction response
  -> persist validated response
  -> create a new ActiveRunControl
  -> start generation N + 1 for the same Run identity

UserAction Cancel
  -> invoke the same interrupt_turn command as the normal Stop control

interrupt while WaitingUserAction
  -> atomically persist Cancelling and cancel PendingUserAction
  -> do not call Harness because no live Run work exists
  -> close the waiting Tool block and persist RunCompleted(Cancelled)
```

UserAction records, response validation, one-time grants, and recovery remain in Service and UserActionStore. `AgentCancellation` is not stored and is not used as the UserAction fact source.

Continuation must compare the same Run, message, block, call, action, and invocation-generation identities. Once the Turn enters `Cancelling`, no UserAction response may restart it. Service creates a new `ActiveRunControl` only when an exactly-once response transition has produced a validated durable continuation; waiting state alone never recreates an in-memory control, including after process restart.

The Service and Harness transitions are separate:

```text
Harness: Running(generation N)
  -> settle all Run-scoped work
  -> remove ActiveRunControl
  -> emit WaitingForInput

Service: Running
  -> WaitingUserAction(pending identities, continuation N)
  -> Running and start Harness generation N + 1

Service: WaitingUserAction
  -> Cancelling
  -> Completed(Cancelled), without a Harness interrupt
```

An unexpected live `ActiveRunControl` while durable state is `WaitingUserAction` is an invariant violation, not a continuation path. Service must interrupt that exact control and execute bounded recovery before allowing a response or a newer Turn.

## 14. Service Persistence and Multi-Client API

Clients access product APIs, never Agent Harness directly:

```text
interrupt_turn(InterruptTurnRequest) -> InterruptTurnResponse
acknowledge_cleanup_risk(AcknowledgeCleanupRiskRequest) -> AcknowledgeCleanupRiskResponse
get_active_turn(SessionActiveTurnRequest) -> TimelineActiveTurnSnapshot
subscribe_timeline(SubscribeTimelineRequest) -> TimelineSubscriptionSource
recover_timeline(TimelineRecoveryRequest) -> TimelineRecoveryResponse
```

Timeline keeps Agent execution and product state explicit:

```rust
pub enum TimelineEventPayload {
    Agent {
        selection: AgentSelection,
        event: AgentEvent,
    },
    TurnStatusChanged {
        status: ActiveTurnStatus,
    },
    SessionExecutionGateChanged {
        gate: SessionExecutionGate,
    },
}
```

`TurnStatusChanged` is a Service-owned product event, not another Agent lifecycle event. It lets every subscribed client observe `Cancelling` immediately while recovery continues to use ActiveTurnStore as the durable status source.

`SessionExecutionGate` prevents a newer Turn from running while an older Run may still mutate shared state:

```rust
pub enum SessionExecutionGate {
    Open,
    CleanupUnconfirmed {
        turn_id: String,
        cleanup_failure_id: String,
    },
}

pub struct AcknowledgeCleanupRiskRequest {
    pub session_id: String,
    pub turn_id: String,
    pub cleanup_failure_id: String,
}

pub struct AcknowledgeCleanupRiskResponse {
    pub gate: SessionExecutionGate,
}
```

The gate is durable Session execution state, not an in-memory scheduler flag, and is included in Session recovery and active snapshots. `cancellation_unconfirmed` atomically writes the failed terminal Turn and `CleanupUnconfirmed`; newer product Turns remain queued. A trusted late owner confirmation may reopen the gate automatically. Otherwise an authorized actor must call `acknowledge_cleanup_risk` with the exact failure identity. Acknowledgement records acceptance of residual side-effect risk; it does not rewrite the failed Run or claim cleanup was confirmed. Clearing the gate and publishing `SessionExecutionGateChanged(Open)` occur in one transaction before product queue dispatch resumes.

The request includes `session_id` and the expected `turn_id`. If a transport retains a session-only convenience command, Service must resolve the active Turn and pass the resolved `turn_id` through every lower call.

`InterruptTurnResponse` expresses acceptance, not completion:

```rust
pub struct InterruptTurnResponse {
    pub session_id: String,
    pub turn_id: String,
    pub status: ActiveTurnStatus,
}
```

The normal Service flow is:

```text
1. Authenticate actor and load Session.
2. Load and compare the expected active Turn identity.
3. Conditionally persist Running | WaitingUserAction -> Cancelling.
4. Cancel the matching PendingUserAction when present and append `TurnStatusChanged(Cancelling)`.
5. If the previous state was Running, call Coordinator/Harness interrupt with stable identity; if it was WaitingUserAction, schedule Service-owned terminalization without calling Harness.
6. Return status Cancelling without waiting for cleanup.
7. Consume the Harness terminal event, or produce the waiting-state cancellation terminal event in Service.
8. Persist item closures and terminal Timeline event.
9. Persist ActiveTurn Completed or Failed and update the Session execution gate when required.
10. Publish committed events; start the next product Turn only while the gate is `Open`.
```

Step 3 must occur before the cancellation signal so a fast terminal event cannot race ahead of the durable cancelling state. ActiveTurn transition, matching PendingUserAction cancellation, and the product status event execute in one orchestration-store transaction. Terminal Timeline append, item closure, ActiveTurn terminal transition, and any Session execution gate change execute in another idempotent transaction keyed by the exact Turn and terminal event identity. Live publication occurs only after commit.

After the durable cancellation transaction commits, the cancellation intent is accepted even if live delivery races with completion:

- `Accepted` or `AlreadyCancelling`: return durable `Cancelling`.
- `RunNotFound` for a previously Running Turn: reload durable state; return an existing terminal state or execute exact-Turn orphan recovery.
- a previously `WaitingUserAction` Turn does not call Harness; Service owns its cancellation terminalization.
- identity conflict: fail without touching the different active Turn.
- an unexpected in-process interrupt failure: persist a typed failed terminal recovery for the exact Turn; never roll back to `Running`.

Harness interrupt performs only in-memory identity comparison and cancellation signalling. It performs no provider, Tool, network, or store I/O, so this call has its own short bounded delivery deadline.

Harness does not queue a new product Turn. Any existing same-Run continuation waiting in the Harness backlog is rejected and its response or stream channel is settled when the Run enters `Cancelling`; it must never be dispatched by cancellation cleanup. Admission of a newer product Turn is Service-owned and depends on durable terminal state plus the Session execution gate.

The durable facts are:

| Fact | Owner |
| --- | --- |
| Live cancellation token, abort handle, completion signal | Agent Harness |
| Active Turn status and current action ID | ActiveTurnStore |
| Session execution gate and cleanup failure identity | SessionStore |
| Pending action, response, and action state | UserActionStore |
| Stable events, ordering, and replay cursor | TimelineStore |
| Client presentation state | Derived from active snapshot and Timeline |

Web, Desktop, TUI, CLI, and SDK receive the same asynchronous semantics:

- response `Cancelling` means the request was accepted;
- clients remain busy and keep the Timeline subscription open;
- only a persisted final event moves the client to idle;
- reconnect and refresh reconstruct state from ActiveTurn plus Timeline;
- a `CleanupUnconfirmed` gate disables new execution and exposes the exact failed Turn until it is resolved;
- no client invents a local cancellation terminal event.

## 15. Durable State and Event Reduction

The product state machine is:

```text
Running ------------------------------> Completed / Failed
   |
   +-- interrupt --> Cancelling ------> Completed(Cancelled)

WaitingUserAction --------------------> Running
   |
   +-- interrupt --> Cancelling ------> Completed(Cancelled)
```

State rules:

1. `Running | WaitingUserAction -> Cancelling` is a conditional update for the exact Turn.
2. Repeated interrupt while `Cancelling` is idempotent.
3. Non-terminal events for the same Run cannot change `Cancelling` back to `Running`.
4. No event can mutate an already terminal Turn.
5. Completed and failed Tool items keep their terminal state during Run cancellation.
6. Queued, running, or waiting items become cancelled after their work or wait is terminated.
7. A late Runtime, Tool, or Provider event from a closed invocation generation may be traced for diagnostics but cannot reopen a block, message, Run, or Turn.
8. A queued same-Run continuation is rejected when cancellation starts and cannot create a new invocation.
9. Confirmed cleanup, including confirmed force cleanup, leaves the Session execution gate open.
10. Unconfirmed cleanup atomically blocks Session execution with the failed terminal transition.
11. The next product Turn starts only after terminal persistence, live Run settlement, and an open Session execution gate.

Harness owns terminal events for live Runs. Service owns waiting-state cancellation and durable restart recovery when a previously Running Turn has no live `ActiveRunControl`. Orphan recovery must close active items and write one explicit terminal failure or cancellation outcome; it cannot write only a generic Turn error while leaving items running.

Harness maintains one private per-Run event reducer for event validation, terminal output materialization, and force-abort closure of open Agent message blocks. Harness emits canonical block-completion and terminal Agent events; Service alone projects and persists product Timeline item closures. The reducer is not a second message model or public event bus.

If Service persists `Cancelling` and the Agent handle disappears concurrently, it does not roll back to `Running`. It reloads current durable state and completes orphan recovery for the exact Turn.

## 16. Streaming, Completion, and History

Unary and streamed execution share one completion contract:

```text
AgentHarness::run(params) -> AgentResult<AgentRunResult>
AgentHarness::run_streamed(params) -> AgentResult<AgentRunStream>
unary Completed(output) == terminal RunCompleted(output)
unary Failed(failure) == terminal RunFailed(failure)
```

`AgentRunStream::completion()` carries only in-process checkpoint metadata required by Service. It is not serializable and does not enter `AgentEvent`, Timeline, SDK, or history.

Waiting input is a Runtime-invocation completion, not a logical Run or product Turn completion. Harness emits `WaitingForInput` without `RunCompleted` only after closing the invocation sink, confirming no Run-scoped work remains, and removing `ActiveRunControl`. Service preserves the unfinished message, block, action, checkpoint, and generation identities. A validated response starts another Runtime invocation for the same Run with a new control and generation. The Turn-scoped product subscription stays active across the no-Runtime waiting period until Service cancellation, failure, or final output produces the Run's single terminal event.

History uses:

```rust
pub struct SessionHistoryTurn {
    pub turn_id: String,
    pub input_message_id: String,
    pub result: AgentRunResult,
}
```

Live, replay, terminal output, and history contain the same completed or bounded partial messages. Store records remain private and Service performs the single history projection.

## 17. Coordinator, Kernel, and Provider

Coordinator owns Agent selection and forwarding only. It accepts routing metadata plus canonical Agent params and never receives a full product Session merely as a convenience bag.

```rust
pub struct AgentDispatchParams {
    pub binding: DispatchBinding,
    pub routing: AgentRoutingContext,
    pub run: AgentRunParams,
}
```

Lifecycle forwarding uses the same stable Run identity. Coordinator does not inspect `ActiveRunControl`, Runtime kind, cancellation token, Tool state, or backend handles.

Agent Kernel owns `AgentKernelInput`, `AgentKernelOutput`, `AgentKernelEvent`, Kernel messages, Kernel continuation, and Kernel persistence contracts. Kernel loop state, processors, schedulers, builders, and transcript helpers become private implementation modules.

Kernel Runtime owns the only Agent-to-Kernel projection. Cancellation is projected into the Kernel execution context and then into Tool execution context; Kernel does not import Harness lifecycle objects.

LLM Provider keeps `ChatRequest`, `ChatResponse`, and `ModelBlockStreamEvent`. Raw SDK and SSE objects remain private. Provider, Kernel, Agent, and product metrics reuse `talos-common::TokenUsage` without another normalized usage DTO.

## 18. Error Semantics

| Layer | Stable errors |
| --- | --- |
| Provider | Provider-neutral request, stream, authentication, and availability errors |
| Kernel | Input, context, model/tool execution, continuation, and compaction errors |
| Runtime adapter | One projection from backend errors into `AgentError` |
| Harness | Run conflict, Run not found, input rejected, interrupt rejected, cleanup timeout |
| Coordinator | Agent routing, binding, disabled Agent, capability errors |
| Service | Product state, authorization, persistence, recovery, and transport mapping |

Cancellation outcomes:

| Condition | Agent terminal result |
| --- | --- |
| Cooperative cleanup completed | `RunCompleted(Cancelled)` |
| Cooperative cleanup confirms all locally owned resources stopped | `RunCompleted(Cancelled)` |
| Grace deadline forces Runtime abort and operation cleanup is confirmed | `RunFailed(forced_abort)` |
| Any local or side-effecting remote operation lacks cleanup confirmation | `RunFailed(cancellation_unconfirmed)` and block Session execution |
| Run already terminal | Service returns the durable terminal state; Harness has no live control |
| Requested identity does not match active Run | `RunNotFound` or `RunConflict` |

No layer parses error messages or silently falls back to another Runtime, Agent, model, Provider, legacy decoder, or new Run.

## 19. Breaking Changes and Removal

This refactor is a coordinated breaking replacement:

Implementation must be split in a separate `IMPLEMENTATION_PLAN.md` into build-green, testable ownership migrations. Each phase completes its current boundary and quality gates before the next begins. The final target has no dual wire schema or compatibility path; temporary branch-local plumbing must be removed before its phase is marked complete.

| Remove or relocate | Target |
| --- | --- |
| `TalosInputItem`, duplicate active-run input enums | `AgentInput` and `AgentUserMessage` |
| legacy `AgentRunResult` struct with parallel output fields | `AgentRunResult::{Completed, Failed}` and canonical message snapshots |
| expanded Coordinator result fields | nested `AgentOutput` plus `AgentSelection` |
| product-owned Agent/Turn/Item event unions | `AgentEvent` and typed message-block lifecycle |
| `RunItem`, `TalosTimelineItem`, overlapping history message DTO | `AgentMessage` and `SessionHistoryTurn` |
| public Kernel stream/loop/building types | cohesive Kernel facade and private modules |
| `AgentKind` Runtime placeholders | implemented-only `AgentRuntimeKind` |
| Provider-local normalized usage | `talos-common::TokenUsage` |
| full Session in Coordinator/Harness execution params | routing context, Agent params, and narrow identity |
| Harness-owned Session title state | Service-owned title policy and persistence |
| abort-only active Turn handle | `ActiveRunControl` |
| detached Bash reader/waiter/timeout tasks | Bash-local structured supervisor |
| client-side cancellation terminal inference | persisted ActiveTurn and Timeline terminal events |

No removed name survives as an alias, deprecated field, compatibility re-export, dual event stream, or fallback parser.

Persisted data requires deterministic migration:

1. Version affected Timeline, history, active-Turn, UserAction, idempotency, and transcript schemas.
2. Settle or recover active Runs before transforming partial event streams.
3. Preserve Session, Turn, input-message, message, block, call, action, timestamp, sequence, and cursor identity.
4. Preserve waiting actions and accepted responses so continuation remains exactly once.
5. Regenerate Rust schema and TypeScript bindings and update all clients in the same protocol version.
6. Fail startup on missing or partial migration; do not fall back to legacy decoding.
7. Keep ordered migration history permanently while deleting old runtime readers and writers.

## 20. Impact Area

| Area | Required change |
| --- | --- |
| `talos-agent-api` | Canonical Agent contracts, interrupt params/result, Runtime cancellation IO, narrow supervisor registration port |
| `talos-agent` | Replace abort-only handle with `ActiveRunControl` and bounded supervisor |
| `talos-agent-runtime-kernel` | Project cancellation and canonical Agent/Kernel contracts |
| `talos-agent-kernel` | Private loop internals, structured operation cancellation, terminal block closure |
| `talos-tools` | Cancellation in `ToolContext`; Tool-owned cleanup contract |
| Bash runtime | Local task ownership, process-group termination, joined cleanup, no residual session |
| Model/MCP/Subagent operations | Bounded cancellation of Run-scoped requests without closing application resources |
| Coordinator | Forward narrow Run lifecycle identity and canonical contracts |
| Service | Conditional `Cancelling`, UserAction cancellation, terminal persistence, Session execution gate, orphan recovery |
| Session/ActiveTurn/UserAction/Timeline stores | Exact identity CAS, cleanup gate, and terminal recovery semantics |
| Gateway/App Server/SDK | Asynchronous interrupt and cleanup-risk acknowledgement APIs plus canonical generated contracts |
| Web/Desktop/TUI | Keep cancelling until persisted terminal event; expose blocked cleanup state; recover from server facts |
| History and migration | Canonical messages, preserved identity, no fallback reader |

## 21. Validation Strategy

### 21.1 Agent and Runtime Contract Tests

- Unary output equals terminal streamed output.
- Every completed streamed message equals the same-ID terminal output message.
- Two deterministic fake Runtimes satisfy the same Harness API without Runtime-kind branches.
- Interrupt returns quickly and is idempotent.
- Root cancellation reaches model, Tool, MCP-request, and Subagent child scopes.
- Cooperative completion, grace timeout, force abort, and unconfirmed cleanup produce the specified terminal result.
- Outer `AgentResult::Err` is possible only before `RunStarted`; every later Runtime error becomes the one streamed and unary `AgentRunFailure`.
- Exactly one terminal event is emitted; a final invocation removes its control after terminal delivery, while a waiting invocation removes it before `WaitingForInput` delivery.
- Late events cannot mutate terminal state.
- Supervisor registration reserves bounded ownership before its factory can start a resource, and closed registration starts nothing.

### 21.2 Tool and Bash Integration Tests

- Cancelling a Turn during a parallel Tool batch stops every active child and closes queued items.
- Completed Tool items remain completed.
- Bash cancellation terminates the process group, joins reader/waiter/timeout tasks, and removes its session.
- A force-aborted Runtime cannot leave a Bash child process running.
- Long blocking work cannot enter the Run path without a cooperative or process-level stop contract.
- MCP connection remains usable after its Run-scoped request is cancelled.
- Subagent cancellation reaches the child started by the Run without affecting unrelated Runs.

### 21.3 Service and Store Tests

- Exact Turn identity is required for `Running | WaitingUserAction -> Cancelling`.
- Repeated interrupt is idempotent and a stale interrupt cannot cancel a newer Turn.
- `Cancelling` is not overwritten by late progress or item events.
- Waiting UserAction cancellation closes the action and Run without a live Runtime task.
- Waiting UserAction cancellation does not call Harness or recreate `ActiveRunControl`.
- Response submission cannot resume a cancelling Run.
- Missing live control for a previously Running Turn invokes bounded orphan recovery and closes active items.
- Cancelling a Running Run rejects every queued same-Run continuation before cleanup can dispatch it.
- Confirmed force cleanup allows queue progress after terminal persistence.
- Unconfirmed cleanup durably blocks newer product Turns until trusted confirmation or exact-risk acknowledgement.
- Cleanup-risk acknowledgement is authorized, identity-checked, auditable, and does not rewrite the failed Run.
- Restart and refresh reconstruct cancelling or terminal state from stores.

### 21.4 Protocol and Client Tests

- HTTP, App Server, Embedded SDK, Web, Desktop, TUI, and CLI expose the same interrupt semantics.
- Interrupt response reports `Cancelling`, not false terminal success.
- Clients remain busy until the persisted terminal event.
- Live, replay, recovery snapshot, and history render identical block states.
- Schema generation contains no duplicate Agent or cancellation DTOs.

### 21.5 Architecture Audits

- Normal-feature `cargo tree` satisfies the package dependency boundaries.
- Agent API and Kernel crate roots match explicit public allowlists.
- No full `TalosSession`, store record, transport object, Runtime handle, or cancellation token crosses a forbidden boundary.
- No detached Run-scoped task remains in Tool and Runtime production paths.
- Residual searches find no removed DTO, legacy event, compatibility alias, or duplicate client business union outside migration history.

## 22. Final Invariants

1. Coordinator routes Agents; Harness owns Agent Run lifecycle; Runtime adapts a backend.
2. Service owns durable product state and is the only multi-client fact source.
3. One product Turn maps to one logical Agent Run; waiting may split it into multiple Runtime invocations.
4. One Session contains at most one exact-identity `ActiveRunControl`, and only while a Runtime invocation or its cleanup is live.
5. Interrupt cancels the full Run, not one Tool, and returns acceptance without waiting for cleanup.
6. Run cancellation is hierarchical, idempotent, deadline-bounded, and terminal exactly once.
7. Tools own cleanup of their tasks, streams, requests, and processes.
8. No confirmed Run-scoped work remains after `RunCompleted(Cancelled)`.
9. Confirmed force cleanup fails as `forced_abort`; unconfirmed cleanup fails as `cancellation_unconfirmed`.
10. UserAction is durable waiting state without `ActiveRunControl`; its Cancel action invokes Service-owned Turn interrupt handling.
11. `Cancelling` cannot regress to `Running`, and terminal state cannot be reopened by late events.
12. Queued same-Run continuations are rejected on cancellation; the next product Turn starts only after durable terminal persistence, live Run settlement, and an open Session execution gate.
13. `AgentMessage` and `AgentEvent` are the only Agent-level conversation and event contracts.
14. Runtime-private, Kernel-private, Provider-wire, store, and transport objects stop at their owners.
15. No parallel lifecycle framework, task registry, resource registry, event bus, or fallback compatibility path is introduced.
