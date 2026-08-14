# Agent Runtime Message and Event API Standardization

Status: draft
Owner: agent
Created: 2026-08-11
Updated: 2026-08-14
Related: `feat-032-session-subagent`, `feat-033-user-file-storage-model-input`, `refactor-038-llm-model-selection`

## 1. Decision Summary

Talos keeps the existing `AgentRuntime` concept. No parallel `Engine` abstraction is introduced.

The default execution path has six stable responsibilities:

```text
Talos Service
    -> Agent Coordinator
        -> Agent Harness
            -> AgentRuntime implementation
                -> Agent Kernel
                    -> LLM Provider
```

An Agent Coordinator selects a registered Agent Harness. Each Harness is constructed with one `AgentRuntime` implementation. The default runtime is Kernel-backed; future runtimes may be backed by Codex, Claude Code, or another agent implementation. The Coordinator never selects or inspects a runtime directly.

The main Agent API is standardized around:

```text
AgentInput
AgentRunParams
AgentOutput
AgentEvent
AgentMessage
```

`AgentInput` is the caller-provided semantic input. `AgentRunParams`, `AgentOutput`, and `AgentEvent` are invocation contracts. `AgentMessage` is the stable conversation object reused by input, output, history, SDKs, and clients. Agent Runtime implementations use the same Agent-level contracts when communicating with Agent Harness; they do not introduce mirror `AgentRuntimeInput`, `AgentRuntimeOutput`, or `AgentRuntimeEvent` objects.

Each lower independent library continues to own its own contract:

```text
Agent Kernel: AgentKernelInput / AgentKernelOutput / AgentKernelEvent
LLM Provider: ChatRequest / ChatResponse / ModelBlockStreamEvent
```

Objects move upward by direct reuse, composition, or semantic projection. Raw Provider SSE chunks, Provider-private response objects, Kernel loop state, and Kernel trace events stop at their owning boundary. Lower-level objects are not propagated to every higher layer merely to preserve their original form.

## 2. Background and Problems

Talos already has the intended layers, but its current public objects do not consistently follow them:

1. Initial user input uses `Vec<TalosInputItem>`, while active-run input uses a separate `AgentDispatchInput` and then an almost identical `AgentInput`.
2. `AgentDispatchInput` is converted to `AgentInput` by exhaustive field copying even though the semantic payload is unchanged.
3. Kernel execution produces `GeneralTurnResult`, Runtime converts it to `AgentRunResult`, and Coordinator expands that result again into `AgentDispatchResult`.
4. Kernel streaming uses both `KernelEvent` and `KernelStreamEvent`, while Runtime converts the latter into product-owned `AgentEvent` values.
5. `AgentEvent`, `RunItem`, `TalosTimelineItem`, and `SessionHistoryMessage` represent overlapping output and message concepts.
6. Agent API objects are stored in the product-oriented `talos-types` crate and re-exported by `talos-agent`, so public ownership is opposite to the actual library boundary.
7. `talos-agent-kernel` exposes many preparation, loop, processor, persistence, and timeline implementation types rather than one cohesive Kernel facade.
8. `AgentKind::Kernel`, `AgentKind::Codex`, and `AgentKind::Claude` describe Runtime families, not different meanings of an Agent identity.
9. Provider `Usage` and product `TokenUsage` represent the same normalized counters with different field names, forcing a conversion at every model-call aggregation boundary.

The result is a pipeline that works, but callers must understand multiple names for the same fact and each layer can accidentally become coupled to a lower implementation detail.

## 3. Goals

1. Keep `AgentRuntime` as the single Harness-to-backend execution abstraction.
2. Allow one Agent Harness implementation to use Kernel, Codex, Claude Code, or another Runtime without changing Coordinator, Service, Gateway, SDK, or UI contracts.
3. Establish one Agent-level input, output, message, and event model owned by the Agent Harness API.
4. Establish one cohesive public input, output, and event model for Agent Kernel.
5. Preserve the existing provider-neutral LLM Provider API and keep Provider wire details private.
6. Retain Coordinator as the necessary Agent routing and forwarding layer without allowing it to redefine Agent payloads.
7. Reuse lower objects directly when their semantics remain unchanged.
8. Use composition when an upper layer adds stable metadata or behavior around a lower object.
9. Use explicit projection when semantics change across a boundary.
10. Stop internal and transport-specific objects at the lowest boundary that fully owns them.
11. Make unary output, streamed completion, persisted history, SDK objects, and UI state converge on the same `AgentMessage` representation.
12. Make each independent library suitable for future third-party use without requiring Talos Service, Gateway, stores, or product configuration objects.
13. Enforce the target module boundaries in the physical Cargo dependency graph, not only through logical ownership documentation.
14. Delete duplicate DTOs, parallel event models, field-copy mappings, unsupported Runtime placeholders, product test helpers, and obsolete public re-exports after their canonical replacements are active.
15. Reduce every core crate to an explicit public API allowlist; implementation algorithms remain private unless a named current external caller requires them.

## 4. Non-Goals

1. This design does not introduce an `Engine` abstraction or rename Runtime to Engine.
2. This design does not add a generic plugin registry for arbitrary third-party Runtime loading.
3. This design does not define Codex or Claude Code Runtime implementation details before those runtimes exist.
4. This design does not expose raw Provider SSE, Provider SDK objects, Kernel loop state, store records, or transport envelopes through the Agent API.
5. This design does not require every upper object to contain the complete lower output or event.
6. This design does not unify management APIs such as Agent definition CRUD, registry administration, or configuration mutation.
7. This design does not add compatibility aliases, dual serialization, fallback parsers, or legacy DTO bridges.
8. This design does not change Agent routing policy or session-to-Agent affinity semantics.
9. This design does not use net line count as a correctness metric. Code is deleted by ownership and reachability; behavior-bearing algorithms are privatized or moved rather than removed merely to reduce LoC.

## 5. Terminology and Ownership

| Concept | Owner | Responsibility |
| --- | --- | --- |
| Agent | Agent Harness layer | Routable configured identity with a descriptor and one selected Runtime |
| Agent Coordinator | `talos-coordinator` | Select and forward to a registered Agent Harness |
| Agent Harness | `talos-agent` | Agent lifecycle, active runs, queues, capability validation, Runtime delegation, and Agent API normalization |
| Agent Runtime | `talos-agent-api` SPI | Adapt one backend agent implementation to the shared Agent API |
| Kernel Agent Runtime | `talos-agent-runtime-kernel` | Convert Agent contracts to and from Agent Kernel contracts |
| Agent Kernel | `talos-agent-kernel` | Model/tool loop, prompt construction, compaction, memory interaction, and Kernel execution policy |
| Shared Common Values | `talos-common` | Dependency-light cross-library values whose semantics are identical at every consuming layer |
| LLM Provider | `talos-llm-provider` | Provider-neutral model request and model stream abstraction |
| Product Service | `talos-service` | Product session validation, persistence, recovery, and orchestration |
| Transport | Gateway/App Server | HTTP, JSON-RPC, SSE, cursor, and protocol envelopes |

The dependency direction is strictly downward:

```text
Gateway / App Server / SDK schema
              |
              v
         Talos Service
              |
              v
      Agent Coordinator
              |
              v
        Agent Harness
              |
              v
         AgentRuntime
          /       \
         v         v
Agent Kernel    future backend adapters
     |
     v
LLM Provider
```

Agent Kernel must not depend on Agent Harness or Coordinator. LLM Provider must not depend on Agent Kernel, Agent Harness, Coordinator, Service, or Gateway. A future non-Kernel Runtime adapter is not required to use the Talos LLM Provider.

`talos-common` is a shared leaf dependency, not another execution layer. Provider, Kernel, Agent API, and product layers may all reuse an approved common value directly without introducing an upward dependency or a conversion DTO.

### 5.1 Target Physical Packages

Logical Agent ownership is implemented with the following physical packages. `talos-agent-api` is a type and SPI package owned by the Agent layer; it is not a new product or runtime concept.

| Package | Owns | Allowed project dependencies | Forbidden dependencies |
| --- | --- | --- | --- |
| `talos-common` | `ModelRef`, `ModelSelection`, `ReasoningEffort`, `ModelSpeed`, `ResolvedModel`, `ModelCapabilities`, `TokenUsage` and other approved cross-library value objects | none | config, Provider, Kernel, Agent, Coordinator, Service, store, Gateway |
| `talos-agent-api` | `AgentMessage`, input/output/event contracts, Harness trait, Runtime SPI, Agent errors and capabilities | `talos-error`, `talos-common` | config, Kernel, Provider, Coordinator, Service, store, tools, memory, Gateway |
| `talos-agent` | Generic Harness implementation, lifecycle, active-run control, Runtime-neutral Agent definition validation | `talos-agent-api`, `talos-error` | config, Kernel, Provider, store, product Service, Gateway; concrete tools/memory dependencies belong to a Runtime adapter |
| `talos-agent-runtime-kernel` | The only Agent API to Agent Kernel projection, default Runtime construction, Kernel persistence/input adapters | `talos-agent-api`, `talos-agent-kernel`, `talos-common`, `talos-llm-provider`, `talos-tools`, `talos-memory`, `talos-store`, `talos-error` | Coordinator, Service, Gateway, client/protocol DTOs |
| `talos-agent-kernel` | Cohesive Kernel facade, model/tool loop, Kernel messages/events, Kernel-owned continuation and persistence SPI | `talos-common`, `talos-llm-provider`, `talos-prompt`, `talos-tools`, `talos-memory`, `talos-error` | config, product-oriented `talos-types`, `talos-agent-api`, `talos-agent`, Coordinator, Service, product store records, Gateway |
| `talos-coordinator` | Agent registry, routing, affinity, selection wrappers and `AgentRoutingContext` | `talos-agent-api`, `talos-router`, `talos-error` | `talos-types`, full product Session objects, concrete Runtime adapters, Kernel, Provider, store, Service, Gateway |
| `talos-service` | Product validation, Turn orchestration, persistence/recovery, history and title policy | Coordinator, `talos-agent-api`, `talos-common`, product stores, memory-domain API; `talos-llm-provider` only for separately owned title generation | concrete Runtime adapters and Agent Kernel execution APIs |
| `talos-store` | Private DB records and store interfaces | `talos-agent-api`, `talos-common`, storage/error infrastructure | Harness implementation, Runtime adapters, Coordinator, Service, Gateway |
| Gateway/App Server | Transport methods, envelopes and generated schemas | Service and generated/public contracts | Kernel, Runtime adapters, store records |
| Product composition root | Construct concrete Kernel Runtime, generic Harnesses, Coordinator and Service | all concrete packages required for assembly | business logic, DTO remapping, Runtime fallback |

The main Agent execution path must satisfy these forbidden edges in `cargo tree`. Existing Service use of Kernel memory helpers moves to the memory domain behind a narrow Service-owned interface; it must not keep `talos-service -> talos-agent-kernel`. Independent Service features may call the provider-neutral LLM API directly only when they do not reuse Agent execution state or Kernel internals.

The allowed-dependency column lists direct Talos packages; external utility crates are omitted. Independence is also a transitive requirement for `talos-common`, `talos-llm-provider`, `talos-agent-kernel`, `talos-agent-api`, and `talos-agent`: their normal dependency closure must not reintroduce product-oriented `talos-types`, `talos-config`, product stores, Coordinator, Service, or Gateway. Therefore the main execution contracts currently imported from `talos-types` or `talos-config` by Provider, prompt, tools, memory, skills, or MCP move to `talos-common` only when their semantics are genuinely cross-library; otherwise they move to their actual domain owner. `talos-memory` owns memory values, `talos-tools` owns tool execution values, `talos-skills` owns skill values, `talos-mcp` owns MCP values, and `talos-prompt` owns resolved prompt options. Product configuration is projected into those resolved inputs at the composition boundary. This refactor does not redesign their CRUD or administration APIs.

`talos-common` is an intentional reuse boundary, not a ban on shared types and not a catch-all crate. A value belongs there only when all of the following are true:

1. At least two independent domain libraries require the same semantics.
2. Callers can reuse the exact object without a domain projection or layer-specific fields.
3. The value has no lifecycle, orchestration, persistence-record, transport-envelope, product Session, or concrete Runtime responsibility.
4. The value can remain stable without depending on a higher domain crate.

These rules keep canonical values shared while leaving `AgentMessage`, `AgentEvent`, Kernel messages/events, Provider requests, memory records, and tool execution objects with their real domain owners. A type is not moved into common merely because two structures currently have similarly named fields.

`talos-common` replaces the reusable-value portion of the current mixed `talos-types`; it is not an additional mirror. A moved value is deleted from `talos-types` and all callers import `talos-common` directly, with no compatibility re-export. Unrelated product protocol and management types may remain in `talos-types` during this refactor because redesigning those APIs is outside scope.

The target source layout keeps central files focused:

```text
talos-common/src/{model,usage}.rs
talos-agent-api/src/{message,event,run,harness,runtime,definition,error}.rs
talos-agent/src/{definition,harness,lifecycle,active_input}.rs
talos-agent-runtime-kernel/src/{runtime,input_projection,event_projection,output_projection,persistence}.rs
talos-agent-kernel/src/api.rs
talos-agent-kernel/src/internal/{run,context,tool_loop,compaction,persistence}.rs
```

Package names and ownership in this section are normative. A source filename may differ only to follow an existing local module convention, with the same single responsibility and no alternative crate placement. Dependency edges are not optional. No central implementation module may absorb message definitions, Runtime projections, lifecycle state, Kernel loop logic, and persistence together.

## 6. Cross-Layer Object Rules

Every cross-layer object relationship must be one of the following:

### 6.1 Direct Reuse

Use the same object when ownership and semantics do not change.

Examples:

- Coordinator forwards `AgentInput` unchanged inside the `AgentRunParams` constructed for the selected Agent Harness.
- Coordinator returns the Harness `AgentOutput` inside a routing result.
- Service persists and publishes each replayable Harness `AgentEvent` inside a recovery envelope; live-only metrics are forwarded unchanged without persistence.
- SDK and UI consume the same `AgentMessage` schema returned by history.

No mapping function is allowed when it only copies every field into a type with another layer prefix.

### 6.2 Composition

Wrap a lower object when the upper layer adds a real responsibility.

Examples:

```rust
pub struct AgentDispatchResult {
    pub selection: AgentSelection,
    pub output: AgentOutput,
}

pub struct TimelineEventEnvelope {
    pub event_id: String,
    pub event_seq: i64,
    pub created_at: i64,
    pub session_id: String,
    pub turn_id: String,
    pub input_message_id: String,
    pub selection: AgentSelection,
    pub event: AgentEvent,
}
```

The wrapper owns only routing, persistence, recovery, or transport metadata. It must not repeat fields already owned by the nested object.

### 6.3 Semantic Projection

Convert an object when the receiving layer has a different abstraction.

Examples:

- Kernel Agent Runtime converts `AgentRunParams` into `AgentKernelInput`.
- Kernel Agent Runtime converts `AgentKernelOutput` into `AgentOutput`.
- Kernel Agent Runtime converts selected `AgentKernelEvent` values into stable `AgentEvent` values.
- Agent Kernel constructs `ChatRequest` from Kernel input, prompt context, tools, memory, and prior messages.
- Service constructs `AgentInput` and `AgentRunParams` from validated product state before dispatch.
- Coordinator selects a Harness with `AgentRoutingContext` and forwards the already complete `AgentRunParams` unchanged.

A projection must have one owner and one direction. Higher layers must not reconstruct lower objects from projected output.

### 6.4 Boundary Stop

Objects stop when their semantics are private to the owning layer.

| Object | Highest allowed boundary |
| --- | --- |
| Provider raw SSE chunk | Concrete Provider adapter |
| Provider wire request/response | Concrete Provider adapter |
| `ModelBlockStreamEvent` | Agent Kernel |
| Kernel loop state and prepared turn state | Agent Kernel implementation |
| Kernel trace/telemetry event | Agent Kernel diagnostics |
| `AgentKernelInput/Output/Event` | Kernel Agent Runtime adapter |
| Runtime abort handle and completion task | Agent Harness |
| `AgentPersistenceCheckpoint` | Talos Service completion handling |
| Store record | Service/store boundary |
| `AgentEvent` | Service, transport, SDK, and clients |
| `TimelineEventEnvelope` | Service, transport, SDK, and clients |

An object may cross its normal stop boundary only when a documented public caller requires that exact stable semantic object. Diagnostic convenience is not sufficient justification.

### 6.5 End-to-End Contract Flow

The main input path is identical for unary and streamed execution:

```text
HTTP / App Protocol StartTurnRequest
    -> Service validates and creates AgentInput plus AgentRunParams
        -> Coordinator selects Agent Harness and forwards AgentRunParams
            -> Agent Harness forwards AgentRunParams to its configured AgentRuntime
                -> KernelAgentRuntime projects AgentRunParams to AgentKernelInput
                    -> Agent Kernel projects execution state to ChatRequest
                        -> LLM Provider adapter projects ChatRequest to provider wire request
```

The output object at each public library boundary is fixed:

| Layer | Unary output | Streamed output | Complete conversation object |
| --- | --- | --- | --- |
| Concrete Provider adapter | `ChatResponse` | `ModelBlockStream`, whose items are `ModelBlockStreamEvent` | None; `ChatResponse` is one model-call result, not a durable conversation message |
| Agent Kernel | `AgentKernelOutput` | Kernel event stream, whose items are `AgentKernelEvent` | `AgentKernelMessage` |
| Agent Runtime | `AgentRuntimeCompletion { output, checkpoint }` | Non-terminal `AgentEvent` values through `AgentRuntimeIo`, followed by the same completion return value | Nested `AgentOutput.messages` are `AgentMessage` values |
| Agent Harness | `AgentOutput` | `AgentRunStream { events, completion }`; events end in `RunCompleted(AgentOutput)` or `RunFailed`, while completion carries only in-process checkpoint metadata | `AgentMessage` |
| Coordinator | `AgentDispatchResult { selection, output }` | `AgentDispatchStream { selection, run: AgentRunStream }` | Nested `AgentOutput.messages` or nested event payloads are unchanged |
| Talos Service | Not used for current main-session commands | `TimelineEventEnvelope { selection, session/turn/input-message identity, event }` | Nested completed `AgentMessage` values are unchanged |
| Gateway / SDK / UI | No separate synchronous Agent answer exists in the current `StartTurnResponse` command API | HTTP SSE or App Protocol notifications carrying `TimelineEventEnvelope` | `MessageCompleted.message`, terminal `RunCompleted(AgentOutput).messages`, and history use the same schema |

Execution mode is selected independently at each boundary. `AgentHarness::run` may internally consume a Kernel or Provider stream and fold it into `AgentOutput`; it does not require Kernel or Provider to use their unary method. Conversely, a non-streaming backend may produce a complete output that Runtime publishes as coarse started/completed Agent events. The invariant is output equivalence, not transport-mode coupling.

Talos main-session commands always execute through `run_streamed`, even when a transport subscriber attaches later, because Service requires lifecycle events, durable waiting state, and completion checkpoint handling. The unary Harness method is the independent library convenience API and contract-test surface; it does not create a second Talos product execution path.

The non-streamed output path is:

```text
Provider wire response
    -> ChatResponse
        -> Agent Kernel aggregates one or more model/tool steps
            -> AgentKernelOutput { messages: Vec<AgentKernelMessage>, ... }
                -> KernelAgentRuntime projects each completed AgentKernelMessage
                    -> AgentOutput { messages: Vec<AgentMessage>, ... }
                        -> AgentHarness::run returns the same AgentOutput
                            -> AgentDispatchResult { selection, output: AgentOutput }
```

On the output path, the first complete lower-library message is `AgentKernelMessage`; the common `AgentMessage` first appears at the Runtime-to-Harness boundary because Runtime owns projection from its backend library into the Agent API. Provider does not create `AgentMessage`. Harness, Coordinator, Service, history, SDK, and UI reuse that complete object. They may wrap it with lifecycle, routing, persistence, or transport metadata, but they do not translate it into another message DTO.

The streamed output path is:

```text
Provider raw SSE
    -> ModelBlockStream { item: ModelBlockStreamEvent }
        -> AgentKernelEvent::MessageStarted / MessageBlockStarted / MessageBlockDelta
            -> AgentEvent::MessageStarted / MessageBlockStarted / MessageBlockDelta
                -> TimelineEventEnvelope { selection, session/turn/input-message identity, event }
                    -> HTTP SSE / App Protocol notification

Provider raw SSE completion
    -> ModelBlockStreamEvent::BlockCompleted / Done
        -> AgentKernelEvent::MessageCompleted(AgentKernelMessage)
            -> KernelAgentRuntime projects the complete message
                -> AgentEvent::MessageCompleted(AgentMessage)
                    -> TimelineEventEnvelope { selection, session/turn/input-message identity, event }
                        -> HTTP SSE / App Protocol notification

AgentKernelEvent::Completed(AgentKernelOutput)
    -> KernelAgentRuntime returns AgentOutput { messages: Vec<AgentMessage>, ... }
        -> Agent Harness emits AgentEvent::RunCompleted(AgentOutput)
            -> TimelineEventEnvelope { selection, session/turn/input-message identity, event }
                -> HTTP SSE / App Protocol notification
```

The `AgentMessage` in `MessageCompleted` is the first complete message for that message lifecycle. `MessageStarted` establishes message identity and role; block events preserve independently rendered text, reasoning, tool, compaction, and status state. Terminal `RunCompleted(AgentOutput).messages` contains the exact latest message snapshots for the invocation. Every completed snapshot has the same ID and content as its prior `MessageCompleted` event. A waiting output may instead contain a message whose tool-call block remains `WaitingForInput` and whose `completed_at` is absent; no false `MessageCompleted` is emitted for it. Stream reducers upsert messages by `message_id` and blocks by `block_id`; they must not append a duplicate when terminal output repeats a message already received through `MessageCompleted`. A durable tool result becomes a tool-role `AgentMessage` and follows the same completed-message rule.

Each arrow is owned by the upper caller or its dedicated adapter. It is not automatically a one-to-one conversion: Provider chunks may be aggregated before a Kernel event, multiple Kernel events may materialize one `AgentMessage`, and Service may add product metadata only by wrapping the unchanged Agent event.

The product `StartTurnResponse` remains a command-acceptance object rather than an Agent answer. Its ambiguous optional `message_id` is replaced by the non-optional `input_message_id`, which is the stable input `AgentMessage.message_id` allocated by Service before dispatch; output messages have their own IDs. Service still waits for the first persisted `RunStarted` event, with the existing timeout and failure handling, before returning accepted status. The completed answer is delivered through timeline events and history. A future synchronous product endpoint, if independently specified, must wrap `AgentDispatchResult` rather than flatten or redefine `AgentMessage`.

## 7. Agent Message Model

`AgentMessage` is the canonical conversation artifact for Agent Harness, Runtime, history, SDK, and UI. It replaces the overlapping message semantics currently spread across `TalosInputItem`, `RunItem`, `TalosTimelineItem`, and `SessionHistoryMessage` without deleting the item-level state required by current clients.

The current `SessionHistoryMessage.message_id` identifies one Talos Turn transcript containing user, assistant, tool, and system items. It is not already a semantic Agent message and must not be mechanically renamed. The target model assigns an ID to each semantic message and groups those messages under the owning Turn only at the Service history boundary.

The target message owns ordered, independently addressable blocks:

```rust
pub struct AgentMessage {
    pub message_id: String,
    pub role: AgentMessageRole,
    pub blocks: Vec<AgentMessageBlock>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

pub enum AgentMessageRole {
    User,
    Assistant,
    Tool,
    System,
}

pub enum AgentAssistantPhase {
    Commentary,
    Unknown,
    FinalAnswer,
}

pub struct AgentMessageBlock {
    pub block_id: String,
    pub parent_block_id: Option<String>,
    pub status: AgentMessageBlockStatus,
    pub title: Option<String>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    pub content: AgentMessageBlockContent,
}

pub enum AgentMessageBlockContent {
    UserInput(AgentUserMessage),
    Reasoning { text: String },
    AssistantText {
        phase: AgentAssistantPhase,
        text: String,
    },
    ToolCall { call: AgentToolCall },
    ToolResult { result: AgentToolResult },
    ExecutionSummary { text: String },
    ContextCompaction { compaction: AgentCompactionMessage },
    SystemNotice { notice: AgentSystemNotice },
    Collaboration { collaboration: AgentCollaborationMessage },
}

pub struct AgentUserMessage {
    pub parts: Vec<AgentUserContent>,
}

pub enum AgentUserContent {
    Text { text: String },
    File { file_id: String },
    Voice {
        url: Option<String>,
        data_base64: Option<String>,
        mime_type: Option<String>,
    },
    Video {
        url: Option<String>,
        mime_type: Option<String>,
    },
}

pub struct AgentToolCall {
    pub call_id: String,
    pub tool_name: String,
    pub arguments: AgentToolArguments,
    pub progress: Vec<AgentToolProgressEntry>,
}

pub struct AgentToolResult {
    pub call_id: String,
    pub tool_name: String,
    pub content: Vec<AgentToolResultContent>,
    pub is_error: bool,
}

pub enum AgentToolResultContent {
    Text { text: String },
    Json { value: AgentJsonValue },
    Collaboration { result: AgentCollaborationMessage },
    FileRead(AgentFileReadContent),
    Diff { files: Vec<AgentFileDiff> },
    SearchResults(AgentSearchResults),
    BashOutput(AgentBashOutput),
    WebReference(AgentWebReference),
    ArtifactRef(AgentArtifactRef),
    McpResourceRef(AgentMcpResourceRef),
    Progress { message: String, elapsed_ms: Option<u64> },
}
```

`AgentUserMessage.parts` preserves the original mixed input order. A legacy `Files` item expands to one `File` part per ID without reordering. File media type is resolved by Service and the attachment reader rather than copied into an untrusted request. An empty parts list and a message containing no meaningful text or attachment remain invalid.

`AgentToolCall`, `AgentToolResult`, and their content items preserve call ID, tool name, typed arguments, rich result content, error state, and bounded progress entries. `AgentToolArguments` and `AgentJsonValue` are Agent-owned validated JSON value objects, not aliases or dependencies on product/tool implementation DTOs. The `AgentFileReadContent`, `AgentFileDiff`, `AgentSearchResults`, `AgentBashOutput`, `AgentWebReference`, `AgentArtifactRef`, and `AgentMcpResourceRef` structures retain every field of their current `TalosContentItem` counterpart; they are relocated and renamed once rather than wrapped in another content DTO. `AgentAssistantPhase` applies only to assistant text and preserves commentary, unknown, and final-answer semantics. Input, reasoning, tool activity, compaction, collaboration, and system semantics come from the typed block content rather than a second generic phase field. `AgentMessageBlockStatus` preserves queued, running, waiting for input, completed, failed, and cancelled states.

The valid role/content matrix is enforced by constructors and deserialization validation: user messages contain user input; assistant messages contain reasoning, assistant text, and tool calls; tool messages contain tool results; system messages contain execution summaries, compaction, notices, and framework collaboration. A generic block envelope is retained because status, timestamps, parent identity, and streaming behavior are shared. This is one model with typed content, not a second timeline DTO.

The canonical mapping is fixed by the following tables. Adding an unlisted catch-all content variant is not allowed.

| Current input | Canonical target | Rule |
| --- | --- | --- |
| `TalosInputItem::Text` | `AgentUserContent::Text` | Preserve item order and text exactly after existing validation |
| `TalosInputItem::Files` | ordered `AgentUserContent::File` parts | Expand IDs in place; preserve files-only input, ownership checks, duplicate rejection and one-files-group validation |
| `TalosInputItem::Voice` | `AgentUserContent::Voice` | Preserve URL/base64/MIME fields and existing size validation |
| `TalosInputItem::Video` | `AgentUserContent::Video` | Preserve URL/MIME fields and existing validation |
| `TalosInputItem::SkillSelection` | `AgentInput.options.skills` | Preserve name/path validation; it is execution intent, not conversation content |

| Current completed content | Canonical message/block |
| --- | --- |
| `RunItem::UserInput`, timeline `UserInput` | User message with one `UserInput(AgentUserMessage)` block |
| `RunItem::Reasoning`, timeline reasoning text | Assistant message with `Reasoning` block |
| `RunItem::OutputText`, assistant timeline text | Assistant message with `AssistantText`; timeline phase selects Unknown, Commentary, or FinalAnswer, and legacy output-only data defaults to FinalAnswer |
| `RunItem::ToolCall`, timeline `ToolCall` | Assistant message with `ToolCall`; result is not embedded in this block |
| `RunItem::ToolResult`, timeline tool result | Correlated Tool message with `ToolResult` block and the same `call_id` |
| `RunItem::ExecutionSummary`, timeline `ExecutionSummary` | System message with `ExecutionSummary` block |
| timeline `ContextCompaction` | System message with `ContextCompaction` block preserving trigger, stage, outcome, metrics, and message |
| timeline `SystemNotice` | System message with `SystemNotice` block preserving level and message |
| `TalosContentItem::SubagentResult` | `AgentToolResultContent::Collaboration` using the existing collaboration domain fields |
| `TalosContentItem::{Text,Json,FileRead,Diff,SearchResults,BashOutput,WebReference,ArtifactRef,McpResourceRef,Progress}` | same-named `AgentToolResultContent` variant | Preserve all current nested fields, truncation flags, offsets, statuses, IDs, MIME data, citations, and timestamps |

| Current stream delta/state | Canonical event/delta |
| --- | --- |
| item started | `MessageStarted` followed by `MessageBlockStarted`; migration may expand one legacy event deterministically |
| `TalosTimelineDelta::Text` | `MessageBlockDelta::Text` applied to AssistantText or Reasoning block |
| `ToolArguments` | `MessageBlockDelta::ToolArguments` |
| `ToolProgress` | `MessageBlockDelta::ToolProgress`; preserve status/stdout/stderr kind and timestamp |
| `ToolResult` | start/update/complete the correlated Tool message; do not retain result inside the ToolCall block |
| `ToolDynamicRequest` | `MessageBlockDelta::DynamicToolRequest` plus ToolCall block status `WaitingForInput` |
| `ContextCompaction` | `MessageBlockDelta::ContextCompaction` |
| assistant-text committed | `MessageBlockDelta::AssistantPhaseChanged` |
| timeline status | `MessageBlockDelta::StatusChanged` |
| item completed | `MessageBlockCompleted`, then `MessageCompleted` when all message blocks are closed |

`TalosTimelineContent::Empty` is transient builder state only. It maps to the typed zero value required by the known item kind while streaming and is never a completed or persisted `AgentMessageBlockContent` variant. `TalosTimelinePhase::{Input,Thinking,Acting,System}` is replaced by typed content; `{Unknown,Commentary,FinalAnswer}` maps only to `AgentAssistantPhase`. `TalosTimelineItemStatus::WaitingDynamicInput` becomes `AgentMessageBlockStatus::WaitingForInput`; all other lifecycle statuses map by meaning. `TalosToolTaskStatus::TimedOut` remains typed tool-result data and maps the owning block to Failed.

Input and output use the same message model:

- Initial text, files, images, voice, and video are blocks in a user message.
- An active-run user message is another user message, not a separate `Message { message: String }` DTO.
- A dynamic tool result is a tool message containing a typed tool-result block.
- Assistant text, reasoning, and tool calls are assistant message blocks.
- Tool results are tool messages correlated to tool-call blocks by `call_id`; clients may visually compose them without changing the domain objects.
- Execution summaries, compaction, and notices are system messages.
- Session history returns Turn wrappers containing unchanged `Vec<AgentMessage>` values.

Skill selection and caller-controlled execution options are not message content. They belong to `AgentInput.options`. Selected Agent context, resolved model configuration, workspace, resume metadata, and trusted execution policy belong to `AgentRunParams`. Talos Session state and routing belong to the upper Service/Coordinator boundary.

## 8. Agent Harness API

The Harness public execution API uses one input, output, event, and message model:

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
    pub model: ModelRef,
}

pub struct AgentRunMetrics {
    pub model_rounds: u32,
    pub tool_calls: u32,
    pub tool_successes: u32,
    pub tool_errors: u32,
    pub usage: TokenUsage,
    pub duration_ms: Option<u64>,
}
```

`AgentRunContext` and `AgentRunPolicy` contain only cross-Runtime Agent semantics. For the current product path, `conversation_id` is the Talos Session identity and `run_id` is the Talos Turn identity; the input identity remains `AgentInput.message.message_id`, and each output message receives a distinct ID. They must not contain `TalosSession`, store records, Gateway requests, the complete product configuration, or Provider SDK objects. Runtime construction settings remain in `AgentRuntimeConfig`; per-run policy contains only resolved, backend-neutral choices and limits. Product Session title state and title generation are Service responsibilities, not Harness API fields.

`AgentOutput.messages` contains every newly materialized message for the invocation in deterministic order. Callers do not also receive parallel `output_text`, `output_items`, and `reasoning` fields. Final assistant text is read from the final assistant message. `AgentRunMetrics` contains token usage plus model/tool counts and duration; usage is not repeated as another `AgentOutput` field. `model` is the provider-neutral `ModelRef` from `talos-common` for the model actually used; the current `AgentDispatchResult.model` value is not dropped or moved into routing metadata. Session identity remains in the upper run context and timeline envelope. All fields ending in `_at` are integer Unix seconds; fields ending in `_ms` are durations, never timestamps.

`AgentOutput.stop_reason` preserves the current externally observable stop semantics: end turn, model output limit, waiting for dynamic input, unresolved transfer, and cancellation. Cancellation is a normal completed output with `AgentStopReason::Cancelled`, not `RunFailed`. A waiting output completes the current Runtime invocation but does not complete the product Turn. `RunFailed` is reserved for execution failures that do not produce a valid completed output.

The Harness event stream is transport-neutral:

```rust
pub struct AgentMessageStarted {
    pub message_id: String,
    pub role: AgentMessageRole,
    pub created_at: i64,
}

pub struct AgentMessageBlockStarted {
    pub message_id: String,
    pub block: AgentMessageBlock,
}

pub struct AgentMessageBlockDelta {
    pub message_id: String,
    pub block_id: String,
    pub delta: AgentMessageBlockDeltaPayload,
}

pub enum AgentMessageBlockDeltaPayload {
    Text { text_delta: String },
    ToolArguments { arguments_delta: String },
    ToolProgress { entry: AgentToolProgressEntry },
    ToolResultContent {
        content: Vec<AgentToolResultContent>,
        is_error: bool,
    },
    DynamicToolRequest { request: AgentDynamicToolRequest },
    ContextCompaction { update: AgentCompactionUpdate },
    AssistantPhaseChanged { phase: AgentAssistantPhase },
    StatusChanged { status: AgentMessageBlockStatus },
}

pub struct AgentMessageBlockCompleted {
    pub message_id: String,
    pub block: AgentMessageBlock,
}

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

pub struct AgentRunStream {
    pub events: Receiver<AgentEvent>,
    completion: AgentRunCompletionReceiver,
}

pub struct AgentRunStreamCompletion {
    pub checkpoint: Option<AgentPersistenceCheckpoint>,
}
```

`AgentMessageBlockDeltaPayload` is a strongly typed union for text, reasoning, tool arguments, tool progress, tool results, compaction updates, assistant-phase changes, and status changes. This replaces both the current timeline delta union and the separate assistant-text-committed event. Every delta is applied to the exact `(message_id, block_id)` pair.

`AgentMessageEvent` is not introduced. Message and block lifecycle are direct variants of `AgentEvent`. `MessageStarted` never carries a partially valid `AgentMessage`; `MessageBlockStarted` carries one valid initial block; `MessageBlockCompleted` carries the full block; and `MessageCompleted` carries the complete `AgentMessage`.

The `talos-agent-api` crate root is limited to the following API families:

```text
message::{AgentMessage, AgentMessageRole, AgentMessageBlock, AgentMessageBlockContent,
         AgentUserMessage, AgentUserContent, AgentToolCall, AgentToolResult}
run::{AgentInput, AgentInputOptions, AgentRunContext, AgentRunPolicy, AgentRunParams,
     AgentOutput, AgentRunMetrics, AgentStopReason, AgentContinuation}
event::{AgentEvent and its typed lifecycle payloads}
harness::{AgentHarness, AgentRunStream, AgentRunStreamCompletion and narrow lifecycle params}
runtime::{AgentRuntime, AgentRuntimeKind, AgentRuntimeIo, AgentRuntimeCompletion,
         AgentPersistenceCheckpoint}
error::{AgentError, AgentErrorKind, AgentResult}
definition::{AgentDescriptor, AgentCapabilities}
```

Types reachable through these contracts remain public in their owning module without being indiscriminately flattened at the crate root. `AgentRuntimeConfig`, concrete Runtime builders, product requests, Session objects, stores, test fakes, and Kernel types are not Agent API exports.

Unary and streaming behavior have one completion contract:

```text
AgentHarness::run(params) -> AgentOutput
AgentHarness::run_streamed(params) -> AgentRunStream
terminal RunCompleted(output) == unary AgentOutput
```

`AgentRunStream::completion()` resolves once after terminal event delivery and returns `AgentRunStreamCompletion` in process. It is Harness/Coordinator/Service plumbing, is not serializable, and is never nested in `AgentEvent` or `TimelineEventEnvelope`. Service consumes the checkpoint for exactly-once progress updates; SDK and replay callers observe only the canonical terminal event.

An interrupted or failed stream emits exactly one terminal event. Open message blocks are completed with cancelled or failed status before that terminal event so clients never retain permanently running items. The Harness does not become idle until terminal event delivery and Runtime completion settlement are complete.

`WaitingForInput` announces the pending request and `RunCompleted` then closes that Runtime invocation with `AgentStopReason::WaitingForInput`. Its output preserves the waiting message/block snapshot without marking either completed. Service must keep the product Turn in its durable waiting state rather than treating that event as final Turn completion. The pending request preserves the same Turn, input-message identity, output message ID, block ID, and call ID for the validated continuation. Submitting its result follows the existing Service validation and persistence path and then resumes the same logical Turn by updating those identities rather than creating replacement placeholders. A later continuation may start only after the prior Runtime invocation has settled, preserving current same-Turn queue ordering. This distinction between invocation completion and product Turn completion is explicit and must not be inferred from stream closure alone.

Harness owns active-run control. Its `submit_input` method accepts an `AgentSubmitInputParams` wrapper containing run identity and the same `AgentInput`; its `interrupt` method accepts stable run identity. Live user messages use the bounded active-run input channel and retain its capacity and conflict errors. A dynamic tool result is not placed on that channel: Service validates the pending action, persists the result, and creates an `AgentRunParams.continuation` for the same logical run. These lifecycle wrappers do not redefine message content.

## 9. AgentRuntime SPI

`AgentRuntime` remains the Harness-owned abstraction for different backend Agent implementations:

```rust
pub trait AgentRuntime: Send + Sync {
    fn kind(&self) -> AgentRuntimeKind;
    fn capabilities(&self) -> AgentCapabilities;

    fn run(
        &self,
        params: AgentRunParams,
        io: AgentRuntimeIo,
    ) -> impl Future<Output = AgentResult<AgentRuntimeCompletion>> + Send;
}

pub struct AgentRuntimeCompletion {
    pub output: AgentOutput,
    pub checkpoint: Option<AgentPersistenceCheckpoint>,
}

pub struct AgentPersistenceCheckpoint {
    pub committed_sequence: u64,
}

pub struct AgentRuntimeIo {
    input_rx: Receiver<AgentInput>,
    event_tx: Sender<AgentEvent>,
    cancellation: AgentCancellation,
}
```

`AgentRuntimeIo` is lifecycle plumbing, not a second semantic input or event contract. Harness constructs it and owns its channels, cancellation state, and completion task. Runtime receives live active-run `AgentInput` values, emits non-terminal `AgentEvent` values through controlled methods, and returns one `AgentRuntimeCompletion` or `AgentError`. Persisted dynamic-tool continuation enters through `AgentRunParams.continuation`, not `input_rx`. Harness alone emits `RunStarted`, `RunCompleted`, and `RunFailed`, so unary and streamed execution cannot disagree about terminal output. Harness interruption operates on its cancellation state rather than leaking a backend handle upward.

`AgentRuntimeCompletion` is not a mirror output DTO. It composes the canonical `AgentOutput` with an optional backend-neutral proof that Runtime-owned durable transcript state was committed through `committed_sequence`. It never contains a backend thread ID, token, path, or store record. Harness returns only `output` from its public unary API. On the streamed path it forwards only the checkpoint through `AgentRunStreamCompletion`; the terminal `AgentEvent` remains pure and replayable. Default Kernel Runtime must supply the checkpoint after transcript commit. Service consumes it to advance existing Session progress exactly once; replayed terminal events cannot repeat that side effect.

There is one Runtime execution method. `AgentHarness::run` executes it with an internal event sink and returns the output. `AgentHarness::run_streamed` executes the same method in a Harness-owned task and publishes the event stream. A non-streaming backend may emit only coarse message completion events, but it still implements the same contract.

Runtime implementations communicate with Harness using Agent contracts. This is intentional standardization, not a loss of Runtime independence. Runtime-specific adapters own conversion to their lower backend library:

```text
KernelAgentRuntime: Agent API <-> Agent Kernel API
CodexAgentRuntime: Agent API <-> Codex agent API
ClaudeCodeAgentRuntime: Agent API <-> Claude Code agent API
```

No `AgentRuntimeInput`, `AgentRuntimeOutput`, or `AgentRuntimeEvent` mirror types are introduced. Runtime-private abort handles, completion tasks, SDK thread IDs, and backend continuation tokens remain inside Runtime and Harness.

`AgentRuntimeKind` identifies the implementation family:

```rust
pub enum AgentRuntimeKind {
    Kernel,
}
```

Only implemented Runtime families are represented. Codex and Claude Code demonstrate future SPI intent in architecture prose; they do not exist as enum variants, config variants, descriptors, capability placeholders, or factory branches until their Runtime implementations and contract tests exist.

The configured Runtime is represented by one tagged configuration object:

```rust
pub enum AgentRuntimeConfig {
    Kernel { kernel: AgentKernelConfig },
}
```

`AgentRuntimeConfig` is the product configuration schema in `talos-config`; it is not part of the dependency-light Agent API package. Product `ConfiguredAgentDefinition` owns this construction config and does not store a separate kind that can disagree with it. The composition root derives `AgentRuntimeKind`, constructs the Runtime, and passes a Runtime-neutral `AgentDefinition` plus the Runtime instance to Harness. No `talos-config` object enters `talos-agent-api` or `talos-agent`.

The Rust implementation must not assume that an RPITIT-based `AgentRuntime` trait can be used as `dyn AgentRuntime`. `AgentHarnessImpl` is generic over its Runtime. The current product composition root builds `KernelAgentRuntime` through `talos-agent-runtime-kernel` and constructs `AgentHarnessImpl<KernelAgentRuntime>` directly:

```rust
pub struct AgentHarnessImpl<R: AgentRuntime> {
    runtime: R,
    // Harness-owned lifecycle state.
}
```

Library and contract tests construct `AgentHarnessImpl<CustomRuntime>` without adding test-only production variants. Coordinator sees only `dyn AgentHarness`. When a second built-in Runtime is fully implemented, the product composition root may introduce a private strongly typed dispatch enum; no one-variant `ConfiguredAgentRuntime`, generic runtime registry, or boxed Runtime interface is added now. A boxed object-safe Runtime interface is allowed only if a real dynamic-loading requirement appears; this Spec does not require one.

Coordinator, Service, Gateway, SDK, and UI may expose the derived Runtime kind as descriptive metadata, but they never branch execution behavior on it.

## 10. Agent Kernel API

Agent Kernel is an independent lower-level library and owns its public execution contract:

```rust
pub struct AgentKernelInput {
    pub context: AgentKernelContext,
    pub message: AgentKernelMessage,
    pub options: AgentKernelOptions,
    pub continuation: Option<AgentKernelContinuation>,
}

pub struct AgentKernelOutput {
    pub messages: Vec<AgentKernelMessage>,
    pub stop_reason: AgentKernelStopReason,
    pub metrics: AgentKernelMetrics,
    pub model: ModelRef,
    pub warnings: Vec<AgentKernelWarning>,
    pub checkpoint: Option<AgentKernelCheckpoint>,
}

pub enum AgentKernelEvent {
    MessageStarted(AgentKernelMessageStarted),
    MessageBlockStarted(AgentKernelMessageBlockStarted),
    MessageBlockDelta(AgentKernelMessageBlockDelta),
    MessageBlockCompleted(AgentKernelMessageBlockCompleted),
    MessageCompleted(AgentKernelMessage),
    WaitingForInput(AgentKernelPendingInput),
    Metrics(AgentKernelMetrics),
    Warning(AgentKernelWarning),
    Completed(AgentKernelOutput),
    Failed(AgentKernelFailure),
}
```

The Kernel root public API is an allowlist:

```text
AgentKernel
AgentKernelInput / AgentKernelOutput / AgentKernelEvent / AgentKernelMessage
AgentKernelContext / AgentKernelOptions / AgentKernelContinuation
AgentKernelStopReason / AgentKernelMetrics / AgentKernelWarning / AgentKernelFailure
AgentKernelPendingInput / AgentKernelCheckpoint
input::{AttachmentReader, ResolvedAttachment, TurnFileScope}
persistence::{KernelTranscriptService, KernelTranscriptRecord, KernelTranscriptFilter}
```

Types reachable as fields of these contracts remain public in the owning `api`, `input`, or `persistence` module, but are not flattened into a large root `pub use` list. Adding another public Kernel type requires a named non-Runtime production caller and a Spec update describing why the facade cannot serve it.

The Kernel facade accepts one structured input rather than many positional arguments and returns one output or event stream. The following current exports become private implementation details: `GeneralTurnResult`, `PreparedGeneralTurn`, `ExecutedTurnArtifacts`, all general-turn loop/step/batch types and functions, request/response processors and pipelines, planner implementations, execution event buffers, tool schedulers/executors, context builders, compaction helpers, persistence helpers, state bags, stream data structs, and timeline builders. They are not deleted when still behavior-bearing; their `pub use` entries and external coupling are deleted.

`AgentKernelMessage` is the complete Kernel-owned message object. It may carry stable Kernel semantics needed by a non-Talos caller, but it must not contain Talos Session, Harness lifecycle, or transport fields. `AgentKernelEvent::MessageCompleted(message)` emits a complete `AgentKernelMessage`; `AgentKernelEvent::Completed(output)` contains the same completed messages in `output.messages`. Kernel stream reducers use Kernel message identity and do not duplicate completed messages at the terminal event. `AgentKernelCheckpoint` proves only that configured Kernel transcript persistence reached a committed sequence; it contains no path or private transcript record. Kernel Agent Runtime maps it to `AgentPersistenceCheckpoint` and does not expose the Kernel type upward.

Kernel message and block content must preserve the behavior currently carried by text/reasoning phases, tool-call arguments, queued/running/progress/result states, dynamic tool requests, context compaction stages, collaboration results, and system notices. `WaitingForInput` preserves a resumable Kernel continuation, and the current invocation completes with a waiting outcome that Runtime maps to `AgentStopReason::WaitingForInput`. Cancellation closes any open Kernel blocks and returns a cancelled outcome that Runtime maps to `AgentStopReason::Cancelled`.

`KernelStreamEvent`, all `*StreamData` payloads used only by it, and the coarse `KernelEvent` execution model are replaced by `AgentKernelEvent`. They are deleted rather than retained as a private parallel event bus. Normal `tracing` spans and fields provide diagnostics; a future structured telemetry contract requires a separate Spec and a real consumer.

Kernel Agent Runtime owns the only projection between Agent and Kernel objects. Agent Kernel does not construct `AgentEvent`, `AgentMessage`, timeline envelopes, or product history objects. The current `TimelineBuilder` is deleted; message/block lifecycle is emitted natively by Kernel and projected by `talos-agent-runtime-kernel`, so no second timeline builder survives.

The current Service caller of `form_daily_session_memory` moves to a narrow memory-domain API. Kernel session compaction remains behind the Kernel facade rather than exposing `compact_session_memory` as a free function. These functions are not exceptions that keep Service or Harness coupled to Kernel internals. Test-only sinks, rejecting readers, mocks, and fixtures move under `cfg(test)` or dedicated test support unless a production example supplies its own local implementation.

## 11. LLM Provider API

The LLM Provider remains an independent library below Agent Kernel. Its current provider-neutral concepts are valid and do not need mechanical renaming solely to match Agent names:

```text
ChatRequest
ChatResponse
ModelBlockStreamEvent
ChatMessage
```

The public Provider contract has one canonical normalized streaming path. Concrete Anthropic, OpenAI, Bailian, DeepSeek, Ollama, and OpenRouter SSE chunks remain private to their adapters.

Provider request/response and stream completion reuse `ModelRef`, `ReasoningEffort`, and `TokenUsage` from `talos-common`. The current provider-local `Usage { prompt_tokens, completion_tokens, ... }` is deleted. Each concrete Provider adapter projects its wire counters once into `TokenUsage { input_tokens, output_tokens, ... }`; Agent Kernel accumulates that same value directly across model rounds, and `AgentRunMetrics.usage` reuses it without another DTO or field-name conversion. `ResolvedModel` and `ModelCapabilities` replace the current product-oriented `ModelInfo` at lower library boundaries; `talos-config` resolves configured model metadata into these dependency-light values before Runtime or Kernel execution.

Moving these values does not move product catalog policy into the common crate. `talos-common` enforces only structural invariants. `talos-config` owns configured catalog construction, and Service validates the requested `ModelSelection` against the selected `ResolvedModel` before dispatch, preserving unknown-model and unsupported-reasoning errors. Runtime and Kernel receive the resolved value and do not repeat catalog lookup or silently downgrade reasoning effort.

Agent Kernel consumes `ModelBlockStreamEvent` and emits Kernel semantics. It does not forward every model event upward. For example:

- Model text deltas may project to `AgentKernelEvent::MessageBlockDelta`.
- Model tool-call blocks may project to Kernel message-block lifecycle events.
- Provider `TokenUsage` is accumulated into Kernel aggregate usage without changing type.
- Provider retry diagnostics remain Provider or Kernel diagnostics unless the Agent caller has an actionable stable requirement.
- Raw Provider payloads, headers, IDs, and protocol-specific stop reasons do not enter Agent events by default.

One Kernel invocation may execute multiple model requests. `AgentKernelOutput` represents the aggregate Kernel result, not a transparent wrapper around one `ChatResponse`.

## 12. Coordinator Routing and Forwarding

Coordinator remains a required orchestration boundary. It owns:

1. Routed or exact Agent binding.
2. Registry lookup and enabled-state validation.
3. Session-to-Agent affinity.
4. Capability validation before dispatch.
5. Returning the selected Agent identity with the Agent result or stream.

Coordinator does not own Runtime selection. Runtime selection is fixed by the selected Agent definition and performed by Agent Harness construction.

The dispatch API adds routing semantics through composition:

```rust
pub struct AgentDispatchParams {
    pub binding: DispatchBinding,
    pub routing: AgentRoutingContext,
    pub run: AgentRunParams,
}

pub struct AgentRoutingContext {
    pub provider: String,
    pub channel_id: Option<String>,
    pub principal_id: String,
}

pub struct AgentDispatchResult {
    pub selection: AgentSelection,
    pub output: AgentOutput,
}

pub struct AgentDispatchStream {
    pub selection: AgentSelection,
    pub run: AgentRunStream,
}
```

`AgentRoutingContext` contains exactly the values consumed by the current routing policy. Coordinator derives the current route channel from `provider` and `channel_id`, routes by `principal_id`, and keys affinity by `run.context.conversation_id`. It does not receive `TalosSession`, `TalosContext`, model configuration, workspace paths, or store records. Service owns the one projection from product Session/context into `AgentRoutingContext` and `AgentRunParams`. Coordinator neither reconstructs `AgentRunParams` nor constructs Harness or Runtime per request.

Active-run input uses the same Agent input object:

```rust
pub struct AgentDispatchInputParams {
    pub binding: DispatchBinding,
    pub routing: AgentRoutingContext,
    pub conversation_id: String,
    pub run_id: String,
    pub input: AgentInput,
}
```

The current `AgentDispatchInput` enum and its field-by-field conversion to the Harness input enum are removed. Coordinator forwards the complete `AgentInput`, including its nested `AgentMessage`, unchanged after routing and product validation.

Session close, clear, delete preparation, interrupt, and explicit compaction use similarly narrow Agent lifecycle params containing stable conversation/run identity plus `AgentRoutingContext` only when routing is required. Service performs product-state validation and projection before calling Coordinator. These existing lifecycle operations are preserved, but no Coordinator or Harness API accepts the full product Session merely as a convenience bag.

## 13. Service, History, and Transport

Public user input reuses the user-message portion of the Agent model. A transport request may restrict accepted roles and add product execution options without defining another content structure:

```rust
pub struct StartTurnRequest {
    pub session_id: String,
    pub message: AgentUserMessage,
    pub options: AgentRequestOptions,
    pub client_request_id: Option<String>,
}

pub struct RetryTurnRequest {
    pub session_id: String,
    pub message: AgentUserMessage,
    pub workspace_id: Option<String>,
    pub client_message_id: String,
    pub retry_of_message_id: String,
}

pub struct AgentRequestOptions {
    pub workspace_id: Option<String>,
    pub context: Option<TalosContext>,
    pub model_selection: Option<ModelSelection>,
    pub project_id: Option<String>,
    pub current_directory: Option<PathBuf>,
    pub skills: Vec<AgentSkillSelection>,
}

pub struct StartTurnResponse {
    pub request_id: String,
    pub input_message_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub status: TurnCommandStatus,
}
```

`AgentUserMessage.parts` carries ordered text/file/voice/video content, so files-only input remains valid while an entirely empty message remains invalid. Image input is represented by a File part whose validated stored media type is resolved by Service. Service authenticates the request, resolves product session state, validates file ownership and options, wraps `AgentUserMessage` in an `AgentMessage` envelope, and constructs `AgentInput`. The role-specific `AgentUserMessage` content is reused directly. Service projects `AgentRequestOptions` once into `AgentInput.options` and trusted `AgentRunContext`/`AgentRunPolicy`; product `TalosContext` and paths do not leak into the lower message object.

`RetryTurnResponse` uses the same response fields. `input_message_id` is intentionally explicit and non-optional because Service allocates it before dispatch, including failed first-event attempts. Retry retains its current narrower option surface and uses validated `client_message_id` as the retried input message identity. `client_message_id`, `retry_of_message_id`, and `client_request_id` retain their separate idempotency and retry meanings and are not aliases for arbitrary output message IDs. Expanding Retry options is outside this refactor.

Service constructs `AgentRunParams` from the unchanged `AgentInput`, neutral run context, resolved cross-Runtime policy, and continuation metadata. Coordinator uses only `AgentRoutingContext` to select the Harness and forwards `AgentRunParams` unchanged. Runtime-specific construction config is consumed once by the product composition root when it constructs the Runtime and Harness; it is not rebuilt from each product Session.

History uses the stable Agent message model while preserving the existing Turn transcript boundary:

```rust
pub struct SessionHistorySnapshot {
    pub session_id: String,
    pub turns: Vec<SessionHistoryTurn>,
    pub generated_at: i64,
    pub last_stable_event_id: Option<String>,
}

pub struct SessionHistoryTurn {
    pub turn_id: String,
    pub input_message_id: String,
    pub messages: Vec<AgentMessage>,
    pub stop_reason: AgentStopReason,
    pub metrics: AgentRunMetrics,
}
```

The current `SessionHistoryMessage` is removed because its name hides that it is a Turn transcript. It is projected into `SessionHistoryTurn`, not mechanically converted into one `AgentMessage`. Each Turn contains the unchanged input message followed by the same completed output messages observed in the live stream and terminal output. Store records and Kernel artifact records remain private and are projected once by Service. Existing history ordering, stable keys, cancelled-placeholder cleanup, artifact reconciliation, stop reason, usage, and metrics remain observable behavior; current standalone usage is nested once as `metrics.usage`, and legacy missing optional metrics migrate to zero counters plus the preserved usage.

Service timeline streaming adds product identity, selection, and recovery metadata:

```rust
pub struct TimelineEventEnvelope {
    pub event_id: String,
    pub event_seq: i64,
    pub created_at: i64,
    pub session_id: String,
    pub turn_id: String,
    pub input_message_id: String,
    pub selection: AgentSelection,
    pub event: AgentEvent,
}
```

The envelope is a real upper-layer composition object: it owns Talos Session/Turn/input-message identity, selected Agent identity, persistence sequence, and replay cursor while preserving the nested Harness event unchanged. `AgentEvent` owns Agent run and output-message lifecycle only and does not repeat Talos Session/Turn fields. HTTP SSE event names are derived from the nested `AgentEvent` variant and are not domain types.

Service allocates the Turn and input-message identities before dispatch. It persists `RunStarted` before acknowledging `StartTurnResponse`, keeps the current first-event timeout behavior, and records idempotency success or failure from that same acceptance point. Retry through `client_message_id` and `retry_of_message_id` retains its current deduplication behavior; canonical message IDs do not replace command request IDs or idempotency keys.

Timeline persistence and recovery semantics remain unchanged: persisted events receive a monotonic `event_seq`, event ID, and timestamp; transient `RunMetrics` is live-only and does not advance the replay cursor; replay and live delivery deduplicate by sequence; retention gaps return cursor-expired; subscriber lag closes the stream for explicit recovery; Turn-scoped subscriptions close only when Service classifies the product Turn as final or failed, while Session-scoped subscriptions continue. A waiting `RunCompleted` settles an invocation but does not close the Turn-scoped subscription. A dynamic tool request sets the product active state to waiting and remains pending even after its Runtime invocation settles.

Product-only Session or Turn events must not be added to `AgentEvent`. If the active main flow gains such an event, Service adds a product timeline variant beside the nested Agent event. It does not force the Harness or Kernel to understand Talos product state.

App Protocol generates TypeScript from Agent-owned contracts. TypeScript SDK, Web, Desktop, and TUI re-export or consume the generated `AgentMessage` and `AgentEvent` types; they do not maintain independent unions.

## 14. Business Behavior Preservation

This refactor changes public type names and wire shapes, but it does not remove or weaken an existing main-session behavior. A concept is deleted only when its semantics are represented by the canonical object at the correct owner boundary.

| Business area | Required preserved behavior | Canonical target representation |
| --- | --- | --- |
| User input | Text, ordered files, image/voice/video references, files-only input, validation, ownership checks, duplicate rejection, model selection, skill selection, workspace, project, actor, and idempotency | `AgentUserMessage`, `AgentInput.options`, and trusted `AgentRunParams` context/policy |
| Agent routing | Routed/exact binding, enabled-state checks, capability checks, session affinity, and selected Agent metadata | Coordinator wrappers around unchanged Agent contracts |
| Run lifecycle | One active start per Session, queued same-Turn continuation, bounded live input, interrupt, drain, close, clear, delete, and explicit compaction | Harness lifecycle state plus `AgentRunParams.continuation` and `AgentRuntimeIo` |
| Streaming content | Input, reasoning, commentary, unknown/final assistant phases, tool lifecycle, execution summaries, compaction, collaboration, and system notices | `AgentMessage` plus addressable `AgentMessageBlock` lifecycle in `AgentEvent` |
| Tool execution | Call ID correlation, typed arguments, queued/running state, stdout/stderr progress, rich result content, errors, diffs, file reads, search results, and bash output | `AgentToolCall`, `AgentToolResult`, and typed block deltas |
| Dynamic input | Durable pending request, Service-side validation, stored response, suspended state, and same logical Turn continuation | `WaitingForInput`, product pending-action state, and `AgentRunParams.continuation` |
| Completion and failure | End-turn, model-limit, transfer-unresolved, cancellation, execution failure, one terminal event, and closure of open blocks | `AgentStopReason`, `RunCompleted`, and `RunFailed` with explicit rules |
| Persistence and recovery | Monotonic event sequence, replay/live deduplication, cursor expiry, lag recovery, retention, subscriber scope, active-Turn recovery, and orphan cleanup | `TimelineEventEnvelope` plus private store records |
| History | Turn grouping, input/output ordering, stable identities, artifact reconciliation, cancelled-placeholder cleanup, stop reason, usage, and metrics | `SessionHistoryTurn { messages: Vec<AgentMessage>, ... }` |
| Session state | Exactly-once progress advancement, message count/model/usage update, active-state transitions, title generation, title persistence, and cache invalidation | Service-owned commit and title policy |
| Subagent | Inherited Agent binding, model, workspace tree, final-answer extraction, interruption/failure state, bounded output, and tree limits | Agent contracts projected into the existing Subagent domain objects |
| Clients | History/live/replay equivalence, local-input replacement, block upsert, status/phase rendering, metrics, and no duplicate terminal messages | Generated canonical contracts plus presentation-only view models |

The client restriction applies to business unions, not presentation state. Web, Desktop, and TUI may keep local view models for selection, scrolling, animation, optimistic rendering, and component layout. Those models must reference or project canonical Agent values and must not redefine message/event semantics.

The existing title feature moves ownership but is not deleted. Service applies the current policy after successful non-cancelled final output: only untitled Sessions are eligible; the selected Provider/model is used; the normalized title is persisted; and the Session cache is invalidated. This logic belongs in the existing Service/Turn flow, not a new manager.

The default Kernel path currently commits private transcript artifacts before completion and carries an internal sequence used by Service to advance Session progress exactly once. That behavior remains through `AgentRuntimeCompletion.checkpoint` and the in-process `AgentRunStreamCompletion`; the sequence never enters `AgentEvent`, `AgentOutput`, wire, schema, SDK, persistence replay, or history. Implementation must prove that transcript commit precedes terminal event publication, duplicate or replayed terminal delivery cannot double-advance progress, and cancelled or waiting invocations do not incorrectly advance completed-message state. Waiting usage remains incomplete and is accumulated under the existing profile accounting rules when the continued Turn finally completes.

Subagent status and output remain a semantic projection, not direct leakage of the complete main-session event model. The projection must continue to derive final output from final-answer blocks, map cancellation to interrupted state, preserve failure and truncation information, and enforce the existing bounded output and tree limits.

Implementation cannot begin destructive type removal until every current non-archive input variant, event variant, timeline content/delta variant, history field, and client reducer branch is assigned to this matrix. Any reachable behavior with no canonical target is a Spec defect, not permission to delete the behavior.

## 15. Public API Ownership

Logical ownership is mandatory even if physical crate movement is staged:

| API | Logical owner |
| --- | --- |
| `ModelRef`, `ModelSelection`, `ReasoningEffort`, `ModelSpeed`, `ResolvedModel`, `ModelCapabilities`, `TokenUsage` | `talos-common` |
| `ChatRequest`, `ChatResponse`, `ModelBlockStreamEvent` | `talos-llm-provider` |
| `AgentKernelInput`, `AgentKernelOutput`, `AgentKernelEvent` | `talos-agent-kernel` |
| `AgentInput`, `AgentRunParams`, `AgentOutput`, `AgentMessage`, `AgentEvent` | Agent layer, physically in `talos-agent-api` |
| `AgentRuntime`, Runtime kind/capabilities, Harness trait | Agent layer SPI, physically in `talos-agent-api` |
| `AgentRuntimeConfig` | `talos-config`; only implemented Runtime variants are valid |
| Generic Harness lifecycle and Agent definition | `talos-agent` |
| Kernel Runtime adapter and its product dependency assembly | `talos-agent-runtime-kernel` |
| Agent selection and dispatch wrappers | `talos-coordinator` |
| Session history and timeline recovery wrappers | `talos-service` / protocol boundary |
| DB records | corresponding `*-store` crate |

Agent public objects must not remain product-owned objects that are merely re-exported by `talos-agent`. `talos-agent-api` is the decided cycle-free physical owner and `talos-agent` re-exports nothing merely for compatibility. Callers import the owning package directly. Product-oriented `talos-types` stops exporting Agent execution, timeline, and history business unions. Genuinely cross-library values move to `talos-common`; the product model-list DTO may retain `ModelInfo`, but Provider, Kernel, Agent, and Coordinator must not depend on that product DTO.

The physical dependency matrix in Section 5.1 is a completion criterion. A logical owner comment, facade re-export, or staged TODO does not satisfy it. Final workspace metadata must show no forbidden edge, and each affected crate root must match its public allowlist.

The affected crate-root allowlists are:

| Package | Root exports |
| --- | --- |
| `talos-common` | `ModelRef`, `ModelSelection`, `ReasoningEffort`, `ModelSpeed`, `ResolvedModel`, `ModelCapabilities`, `TokenUsage` |
| `talos-agent-api` | the API families listed in Section 8; no concrete implementation or config exports |
| `talos-agent` | `AgentHarnessImpl`, Runtime-neutral `AgentDefinition`, and production constructors required by the composition root |
| `talos-agent-runtime-kernel` | `KernelAgentRuntime` and `KernelAgentRuntimeBuilder`; projection modules and concrete input/persistence adapters remain private |
| `talos-agent-kernel` | the allowlist in Section 10 |
| `talos-coordinator` | `Coordinator`, `CoordinatorImpl`, registry/registration contracts, selection values, dispatch wrappers, `AgentRoutingContext`, and narrow lifecycle params |

An export not covered by these rows requires a current non-test external caller and an update to this Spec. Tests and examples import test support from test-only modules or define local fakes; they do not expand production roots.

## 16. Error Semantics

Each independent library owns stable errors for its callers:

- LLM Provider errors describe provider-neutral model request failures and retain private wire context only as an internal source.
- Agent Kernel errors describe Kernel validation, execution, tool, context, and continuation failures.
- Runtime adapters map lower errors once into `AgentError`, preserving a private source chain for tracing.
- Harness adds lifecycle, capability, conflict, interrupt, and active-run errors.
- Coordinator adds routing, missing Agent, disabled Agent, and binding errors.
- Harness maps Runtime errors into the single terminal `AgentEvent::RunFailed` value.
- Service maps Agent errors or failed terminal events into product state and transport error semantics.
- Gateway maps product errors into transport envelopes without parsing error messages.

Raw Provider response bodies, credentials, endpoints, local paths, backend thread IDs, and continuation tokens are never included in public Agent errors or events.

No layer silently falls back to another Runtime, Agent, model, or Provider when the selected implementation fails.

## 17. Persistence and Replay Invariants

1. `AgentMessage` is the canonical persisted conversation representation.
2. `AgentEvent` is the canonical Agent execution representation; its stable variants are replayable and `RunMetrics` is explicitly live-only.
3. Replaying the stable events for a completed run materializes the same messages returned by `AgentOutput`.
4. `AgentEvent::RunCompleted(output)` contains the same `AgentOutput` produced by unary execution.
5. Store records may optimize event and message storage but never become public API objects.
6. Kernel artifacts and transcripts remain Kernel-owned recovery data and are not treated as product history DTOs.
7. Provider responses are not persisted as Agent history unless a specific provider-neutral field is required for continuation.
8. Event ordering, one-terminal-event behavior, and message identity are deterministic across unary, live stream, recovery, and history paths.
9. Every `AgentEvent::MessageCompleted(message)` value is present exactly once by `message_id` in terminal `AgentOutput.messages`.
10. The value in terminal `RunCompleted(output)` is equal to unary `AgentHarness::run` output for the same deterministic execution.
11. Timeline persistence stores the completed `AgentMessage`; started/delta events may be retained for replay but are never treated as an additional message record.
12. `RunMetrics` is live-only, does not advance the persisted cursor, and final metrics remain available through `AgentOutput` and history.
13. A waiting `RunCompleted` settles one Runtime invocation while preserving the product active Turn and pending action for continuation.
14. Turn-scoped subscribers close only for a final or failed product Turn; a waiting invocation does not end the Turn subscription. Session-scoped subscribers remain open across Turns.
15. Default Kernel transcript persistence completes before its terminal event is published, and Service advances Session progress at most once from typed internal commit metadata.
16. A waiting terminal output persists the current message/block snapshot without emitting `MessageCompleted`; continuation updates the same message, block, and call identities.

## 18. Breaking Changes and Migration Scope

This refactor is a breaking replacement with no live compatibility layer:

1. Remove `TalosInputItem` as the Agent execution input model after all product inputs use typed user messages.
2. Remove mirrored `AgentDispatchInput` and the duplicate Harness `AgentInput` enum used only for active-run messages.
3. Replace `AgentRunResult.output_text`, `output_items`, and `reasoning` with canonical `AgentOutput.messages`.
4. Replace `AgentDispatchResult` field expansion with nested `AgentOutput`.
5. Replace public `KernelStreamEvent` with `AgentKernelEvent`.
6. Remove or privatize coarse `KernelEvent` when it has no separate diagnostic responsibility.
7. Replace `RunItem`, `SessionHistoryMessage`, and overlapping timeline content with `AgentMessage` and Agent event payloads.
8. Rename Runtime-family `AgentKind` to `AgentRuntimeKind` and derive it from `AgentRuntimeConfig`.
9. Move Agent API ownership out of product-oriented shared types.
10. Remove product Session title state and generated title fields from Harness contracts after the equivalent Service-owned title policy and persistence path is in place.
11. Regenerate App Protocol schema and TypeScript bindings and update SDK, Web, Desktop, TUI, fixtures, and examples in the same migration.
12. Do not retain aliases, deprecated fields, dual event streams, old history readers, or reverse tests for removed definitions.
13. Replace Provider `Usage` with shared `TokenUsage`, move stable cross-library model values to `talos-common`, and remove lower-library imports of product `ModelInfo`.
14. Replace full-`TalosSession` Coordinator/Harness execution and lifecycle parameters with `AgentRoutingContext`, `AgentRunParams`, and narrow lifecycle params.

### 18.1 Removal Ledger

The following ledger is normative. Implementation updates it only when code search finds an additional current symbol in the same ownership category.

| Action | Current code to remove or relocate | Target |
| --- | --- | --- |
| Delete | `AgentDispatchInput`, its conversion match, duplicate active-run Harness input enum | one `AgentInput` and lifecycle wrappers |
| Delete | `AgentRunResult`, parallel `output_text`/`output_items`/`reasoning`, expanded `AgentDispatchResult` fields and copy mappings | nested `AgentOutput` |
| Delete | product-owned `AgentEventPayload`, `AgentEventType`, `TalosItem*Event`, `TalosTurn*Event`, `TalosAssistantTextCommittedEvent` | `AgentEvent` and typed message/block lifecycle |
| Delete | `RunItem`, `TalosTimelineItem`, `TalosTimelineContent`, `TalosTimelineDelta`, timeline kind/phase DTOs, `SessionHistoryMessage` | `AgentMessage`, block content/deltas, `SessionHistoryTurn` |
| Delete | `KernelStreamEvent`, `KernelEvent`, all event-specific `*StreamData`, stream key wrappers, and their conversion/building paths | native `AgentKernelEvent` lifecycle |
| Delete | Kernel `TimelineBuilder` and Agent-side timeline reconstruction | Kernel lifecycle events plus one Runtime projection |
| Delete | duplicated `AgentKind`, config `AgentKind`, Codex/Claude enum placeholders, false capability descriptors, unsupported factory branches | `AgentRuntimeConfig::Kernel` and derived `AgentRuntimeKind::Kernel` |
| Delete | Provider `Usage`, Provider-to-product token counter conversions, and lower-library use of product `ModelInfo` | `talos-common::{TokenUsage, ResolvedModel}` |
| Delete | full `TalosSession` fields in Coordinator dispatch/input/lifecycle params and Harness lifecycle params | `AgentRoutingContext`, canonical Agent run/input, and narrow identity params |
| Delete | Harness generated-title map, title fields in run/session contracts, `take_generated_session_title` bridge | Service-owned title policy and persistence |
| Delete or test-scope | production exports of `MockAgent`, `VecEventSink`, `EmptySkillRootsResolver`, `RejectingAttachmentReader`, and equivalent helpers with no production caller | test support or example-local implementation |
| Delete | Agent/timeline/history/common-value definitions and wildcard exports in `talos-types`, plus SDK/Web/TUI unions that duplicate Agent message/event business semantics | direct imports from `talos-common`, `talos-agent-api`, or owning generated Agent contracts |
| Privatize | general-turn prepare/loop/step/batch/finalize artifacts, processor pipelines, planners, tool schedulers, context builders, state bags, transcript helpers | Kernel `internal` modules behind `AgentKernel` facade |
| Move | Agent-to-Kernel request/event/output mappings, transcript store adapter, attachment adapter, default Runtime construction | `talos-agent-runtime-kernel` |
| Move | daily memory formation called by Service | memory-domain API independent of Agent Kernel internals |
| Move | main-path memory, tool, skill, MCP and prompt execution values imported from `talos-types`/`talos-config` by lower libraries | their existing owning library, with product config projected once into resolved inputs |
| Retain | actual model/tool loop, compaction, dynamic continuation, transcript durability, rich tool content and recovery behavior | private implementation behind canonical contracts |
| Move | `ModelRef`, `ModelSelection`, `ReasoningEffort`, `ModelSpeed`, and `TokenUsage` from product types | `talos-common`, with callers importing the owner directly and no compatibility alias |
| Retain | product model catalog, compaction value data, and collaboration domain data whose semantics remain product-facing | their current product owner unless another explicit lower caller requires a separate stable contract |

Deleting a root `pub use` while leaving an otherwise unreachable implementation is incomplete. After callers migrate, unreachable modules, constructors, mappings, fixtures, snapshots, generated schema entries, and examples are removed in the same stage. Conversely, a behavior-bearing function listed as Privatize or Move must not be deleted merely because its old public name disappears.

Each affected crate has a final public export allowlist checked by source review. Residual symbol searches are delivery audits, not reverse tests for removed behavior.

Persisted user data is not disposable API compatibility. A deterministic migration is mandatory before the old readers and schemas are removed:

1. Inventory and version every affected SQLite record, event-log payload, session transcript/artifact record, active-Turn record, and cached history representation.
2. Quiesce new Turn starts during migration. Existing running Turns must settle or be recovered through the existing orphan policy before their event rows are transformed; the migration must not reinterpret an in-flight partial stream.
3. Migrate retained timeline events transactionally into canonical envelope/event/message shapes while preserving timestamps, Session/Turn/input-message correlation, pending dynamic actions, cursor continuity, and replay ordering. Convert legacy millisecond timestamps deterministically to integer Unix seconds for canonical `_at` fields; preserve strict same-second order through `event_seq` and deterministic message/block order rather than retaining duplicate millisecond fields. When one legacy event expands into multiple canonical lifecycle events, retain the legacy `event_id` on the last equivalent canonical event, generate deterministic IDs for preceding events, and rebuild a monotonic `event_seq` for the migrated log. This keeps existing event-ID cursors valid without a fallback reader or redirect table.
4. Preserve private Kernel transcript and artifact records when their storage schema is still valid. If their schema changes, migrate them separately and verify that history projection and continuation produce the same result.
5. Migrate active waiting Turns and idempotency records so a previously issued dynamic tool result or retry can continue exactly once after upgrade.
6. Update Rust protocol, generated schemas, TypeScript SDK, Web, Desktop, TUI, tests, and deployed server as one coordinated version. Mixed old/new wire clients are unsupported; there is no dual serialization window.
7. Create a database backup or equivalent rollback checkpoint before destructive migration. Startup fails with contextual error when migration is absent, partial, or invalid; it never silently falls back to a legacy decoder.
8. Permanently retain ordered SQL migration files, schema-version metadata, and deterministic transforms required for a fresh database or direct upgrade from any supported release. Delete old runtime readers/writers, dual-write logic, legacy decoders, temporary deployment commands, fixtures, and compatibility branches once the canonical schema is the only runtime schema. Do not delete migration history and do not retain fallback parsing in normal execution.

Therefore wire/source compatibility is intentionally broken, while persisted conversations, pending work, replay cursors, and user-visible behavior are preserved.

## 19. Impact Area

| Area | Required change |
| --- | --- |
| `talos-common` | Own approved dependency-light values reused unchanged across independent libraries |
| `talos-llm-provider` and adapters | Confirm one normalized model stream, reuse shared `TokenUsage`, and hide raw transport events |
| `talos-prompt`, `talos-tools`, `talos-memory`, `talos-skills`, `talos-mcp` | Own their main-path execution values and remove transitive product-type/config coupling from the independent Kernel closure; management APIs remain unchanged |
| `talos-agent-kernel` | Add cohesive facade and Kernel input/output/event; privatize workflow details |
| `talos-agent-api` | Own dependency-light Agent contracts, Harness trait, Runtime SPI and errors |
| `talos-agent` | Keep generic Harness lifecycle and Agent definition; remove concrete Kernel/store/provider dependencies |
| `talos-agent-runtime-kernel` | Own default Runtime construction and all Agent/Kernel/persistence/input projections |
| `talos-types` | Remove Agent/timeline/history and moved common-value definitions plus wildcard exports; retain unrelated product contracts outside this Spec |
| Agent config | Remove duplicated kind and unsupported placeholders; keep one implemented Runtime config source |
| `talos-coordinator` | Preserve routing; accept only narrow routing/run values; compose selection with Agent output/stream; remove mirrored payload mappings |
| `talos-service` | Build Agent input/run/routing values, own Session title policy, persist Agent messages/events, project store records once |
| `talos-store` | Keep records private; persist canonical message/event data as required |
| Gateway/App Server/channels | Use transport envelopes around Agent contracts, preserve subscription behavior, and regenerate schema |
| TypeScript SDK | Re-export generated Agent message/event contracts |
| Web/Desktop/TUI | Render canonical messages and reduce Agent events without local business unions |
| Subagent flow | Preserve inherited binding/model/workspace semantics and project canonical final output/status |
| Profile/session progress | Preserve usage accounting, exactly-once progress advancement, title persistence, and cache invalidation |
| SQLite migrations/config | Version and migrate retained event/history/active-Turn data; regenerate affected schemas |
| E2E | Cover routed Agent selection, default Kernel Runtime, tools, files, dynamic continuation, interruption, stream/replay/history equivalence, and client rendering |
| Build/deploy/docs | Update workspace dependencies, schema-generation declarations, examples, and coordinated deployment artifacts without archive changes |
| Product composition root | Construct Kernel Runtime and generic Harness explicitly; remove Runtime fallback and DTO mapping logic |

Agent definition CRUD, plugin/skill/MCP administration, authentication, project CRUD, file storage CRUD, memory search APIs, and unrelated deployment behavior are not redesigned. Their existing values are affected only where they enter the main Agent input/tool/context flow listed above. Repository archive directories are explicitly excluded from implementation and residual searches.

## 20. Validation Strategy

### Library Contract Tests

- Verify LLM Provider adapters emit the same provider-neutral model event sequence for equivalent responses.
- Verify Provider unary and streamed completion expose the shared `TokenUsage` and Kernel aggregates that same type across model rounds.
- Verify Agent Kernel unary output equals its terminal streamed output.
- Verify every completed `AgentKernelMessage` appears exactly once in terminal `AgentKernelOutput.messages`.
- Verify Kernel Agent Runtime projects representative Kernel text, reasoning, tool, waiting-input, and compaction progress into Agent events, and maps Kernel completion/failure into `AgentOutput`/`AgentError` for Harness-owned terminal events.
- Verify Runtime adapters do not expose raw Provider events, Kernel loop state, backend handles, or private identifiers.
- Verify Agent Harness unary output equals `AgentEvent::RunCompleted(output)`.
- Verify each streamed `MessageCompleted(AgentMessage)` equals the same-ID value in terminal `AgentOutput.messages`.
- Verify cancellation closes open blocks, returns `AgentStopReason::Cancelled`, and does not emit `RunFailed`.
- Verify waiting emits the pending request and a waiting invocation output without `MessageCompleted`, then continuation updates the same message, block, and call IDs.

### Runtime Substitution Tests

- Use at least two deterministic fake `AgentRuntime` implementations behind the same Harness API.
- Verify Harness lifecycle, capability validation, interrupt, active-run input, and output contracts do not branch on concrete Runtime type.
- Verify unsupported Runtime capabilities fail before backend execution.
- Verify Runtime construction never falls back to Kernel for an unsupported configured Runtime.
- Verify active user input remains bounded and dynamic tool results use the validated continuation path rather than the live channel.
- Verify one active start and queued same-Turn continuation ordering are unchanged.

### Coordinator Tests

- Verify routed and exact bindings select the expected Agent Harness.
- Verify two Harnesses using deterministic custom Runtime implementations receive the same `AgentRunParams` contract and unchanged nested `AgentInput`; no production config placeholder is added for the test.
- Verify Coordinator returns `AgentSelection + AgentOutput` without reconstructing output fields.
- Verify active-run input is forwarded as the same `AgentInput` and nested `AgentMessage` value.
- Verify Coordinator does not inspect Runtime/backend-specific or Provider-specific objects and receives no full product Session.

### Service and Persistence Tests

- Verify text, files, media, tool results, and collaboration data round-trip through `AgentMessage`.
- Verify event persistence and replay materialize the same messages as live execution.
- Verify history returns the same complete objects used by live events.
- Verify reducers upsert terminal output by `message_id` and do not duplicate prior `MessageCompleted` values.
- Verify retry, resume, compaction, interruption, and waiting dynamic input preserve message and run identity.
- Verify store records never appear in Service, protocol, SDK, or UI public types.
- Verify Service allocates the input message ID before dispatch but acknowledges start only after persisted `RunStarted`; timeout and first-event failure retain existing idempotency behavior.
- Verify files-only input, ordered attachments, ownership validation, skill selection, model selection, and workspace context retain current behavior.
- Verify waiting dynamic input survives process restart and resumes the same Turn exactly once.
- Verify event sequence, cursor expiry, replay/live deduplication, lag recovery, retention, and Turn/Session subscriber lifetime.
- Verify transcript commit precedes terminal publication and Session progress, profile usage, title persistence, and cache invalidation occur with the current completion rules.
- Verify the database migration preserves completed history, waiting Turns, idempotency, replay cursors, and private Kernel continuation artifacts.

### Protocol and Client Tests

- Regenerate and review Rust JSON schema and TypeScript bindings.
- Verify HTTP SSE and App Server notifications carry the same `AgentEvent` payload.
- Verify Web, Desktop, and TUI render identical history and live event sequences.
- Verify timeline envelopes own only product identity, Agent selection, cursor, and persistence metadata.
- Verify reasoning, commentary/final-answer phase, tool arguments/progress/rich results, dynamic requests, compaction, collaboration, system notices, live metrics, and final metrics remain visible.
- Verify local optimistic input is reconciled with the server input message and every `(message_id, block_id)` update is stable across live, replay, and history hydration.
- Run the existing HTTP SSE, App Server, embedded SDK, TUI, Web/Desktop, files, tools, Subagent, and recovery E2E paths against the canonical contracts.
- Search all non-archive directories for removed duplicate DTOs, old event names, field-copy mappings, and product-owned Agent API definitions.

The tests above are behavior contracts for retained functionality. Tests and fixtures whose only purpose is to assert a removed API name or legacy wire shape are deleted; no reverse test is added for an intentionally removed definition.

### Architecture and Removal Audits

- Inspect workspace metadata and normal-feature `cargo tree` output to prove every direct and transitive dependency rule in Section 5.1; dev-only test support is reviewed separately and must not alter production API roots.
- Compare each affected crate root against its public API allowlist; wildcard Agent/timeline re-exports and unlisted Kernel exports fail review.
- Audit every row in the removal ledger across Rust, generated schema, TypeScript SDK, Web, Desktop, TUI, examples, fixtures, and docs outside archive.
- Confirm the only production `AgentRuntimeConfig` and `AgentRuntimeKind` variant is Kernel and that no unsupported Runtime branch or false capability descriptor remains.
- Confirm `talos-common` has no project-layer dependency, every exported value satisfies its admission rules, Provider no longer depends on product-oriented `talos-types`, and Provider/Kernel/Agent share one `TokenUsage` definition.
- Confirm Coordinator and Harness execution/lifecycle APIs contain no `TalosSession`, `TalosContext`, store record, or product configuration object.
- Confirm `AgentEvent` serializes identically on live and replay paths and contains no skipped persistence, store, backend, or test-only fields.
- Confirm `MockAgent`, fake Runtime implementations, in-memory sinks, rejecting readers, and empty resolvers are test-scoped or example-local rather than production exports.
- Run dead-code and unused-dependency review after callers migrate; remove unreachable modules instead of suppressing warnings.
- Keep new/changed core modules below the project size limits and split message API, lifecycle, Runtime projection, Kernel loop, and persistence by the target layout.

These are source and dependency audits, not behavior tests asserting that deleted names fail to compile.

## 21. Final Invariants

1. Coordinator routes Agents; it does not select Runtimes.
2. Agent Harness owns one configured `AgentRuntime` and all Agent lifecycle behavior.
3. `AgentRuntime` remains the only Harness backend abstraction.
4. Kernel is the only current built-in Runtime; future Codex or Claude Code support must be implemented as Runtime-backed libraries, not new Agent routing layers or placeholder variants.
5. Runtime implementations use shared Agent contracts toward Harness and their own lower contracts toward their backend library.
6. Agent Kernel and LLM Provider remain independently usable lower-level libraries.
7. `AgentMessage` is the only conversation object used at the Agent API and all higher input, output, history, SDK, and UI boundaries.
8. `AgentEvent` is the only Agent-level live and replay event contract.
9. Lower objects are reused or wrapped only while their semantics remain stable and useful to the immediate caller.
10. Provider raw chunks stop at the Provider adapter, `ModelBlockStreamEvent` stops at Kernel, `AgentKernelEvent` stops at Runtime, and Runtime-private state stops at Harness.
11. Routing and transport wrappers add metadata without copying nested business fields.
12. No removed API survives as an alias, fallback, duplicate schema, fixture, or compatibility branch.
13. `AgentInput` contains caller intent; `AgentRunParams` contains trusted execution context assembled by Service after product validation and forwarded unchanged after routing.
14. Harness and Runtime contracts contain no Talos Session title state, full product Session, or transport recovery metadata.
15. `MessageCompleted(AgentMessage)` is the first complete Agent-level streamed message; started and delta events are not alternate message DTOs.
16. Unary `AgentOutput`, terminal `RunCompleted(AgentOutput)`, completed streamed messages, persisted history, and client state converge by `message_id` and content.
17. API and wire compatibility may break, but no existing main-session behavior, persisted conversation, pending continuation, replay guarantee, or client-visible semantic is lost.
18. A product Turn and one Runtime invocation are distinct lifecycle scopes; waiting completes the invocation without falsely completing the Turn.
19. Current title, progress, usage, compaction, Subagent, and interruption behavior has an explicit target owner and verification path before old code is removed.
20. Physical Cargo dependencies satisfy Section 5.1; logical layering without dependency cleanup is incomplete.
21. Only implemented Runtime variants exist in production config and descriptors; Kernel is the sole current variant.
22. Agent API and Kernel crate roots match explicit public allowlists, and all ledger entries marked Delete are absent outside immutable migration history.
23. `AgentEvent` contains only transport-neutral Agent semantics; persistence checkpoints travel only through the in-process run completion channel.
24. Behavior-bearing algorithms are retained privately or moved, while duplicate DTOs, mappings, event models, placeholders, and test helpers are removed.
25. Provider, Kernel, Agent, and product metrics reuse `talos-common::TokenUsage`; no parallel normalized usage DTO or counter-name conversion remains.
26. Service is the last boundary that owns `TalosSession` and `TalosContext`; Coordinator receives only `AgentRoutingContext`, canonical Agent run/input values, and narrow lifecycle identity.
27. Canonical timestamp fields use integer Unix seconds with `_at` names; `_ms` is reserved for durations.
28. The normal dependency closure of Shared Common Values, LLM Provider, Agent Kernel, Agent API, and Agent Harness contains no product types, product config, stores, Coordinator, Service, or Gateway.
