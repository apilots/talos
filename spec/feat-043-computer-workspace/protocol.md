# Agent outbound RPC contracts

Status: implemented. Registration, Desktop identity, Connect/Harness, Worker environment, configuration/plugins/MCP/OAuth and cloud UI composition are implemented. Review fixes and their current acceptance results are recorded in the root implementation plan and [verification](verification.md).
Parent: [design](design.md). Business ownership remains with the existing Harness and product DTOs.

## 1. Code API surface

All listed capabilities are implemented. Transport clients preserve the existing domain interfaces; the Worker owns environment files and the control plane owns product metadata.

| Owner | API / change | Caller and result |
| --- | --- | --- |
| agent-protocol | Existing AgentHarness unchanged | start/submit_input/interrupt/compact_session/clear_session/close_session/read_workspace_memory/update_workspace_memory/form_dream_memory/shutdown; preserve existing AgentRunExecution, AgentAdmission, AgentRunSettlement and typed errors |
| agent-client | AgentNodeClient and AgentNodeRunner extended | create_node/get_node/rotate_credential hide existing Actor-authenticated management RPC; register/renew/heartbeat/unregister plus Connect. Runner coordinates lease, connection, dispatch and cancellation with the existing Worker adapter |
| agent-client | RemoteAgentClient::connect / from_connection | Return the same AgentHarness for direct/outbound access; private HarnessTransport changes only delivery, retaining admission, accepted execution evidence and typed results |
| agent-client | AgentExecutionConnection::accept / ready / ended / close | Control-side bounded live channels; ingress authenticates and installs clients before ready, and owns Registry withdrawal |
| agent-client | AgentNodeClient::run_execution_connection / AgentNodeRunner::with_execution_adapter | Worker-side carrier dispatches through the existing adapter; Runner installs the lease, drains prior execution and reconnects without replay |
| talos-agent | AgentHarnessRpcAdapter::dispatch_connect / validate_connection_auth / drain_disconnected | Shared direct/outbound handlers validate every existing typed request; reconnect requires confirmed local cleanup |
| talos-base | Existing NodeRpcServer / RemoteNodePublisher extended | Accept Connect, install all advertised Harness handles, publish only after installation; revoke using the same node mutation path. Publish Harness and environment handles together |
| agent-protocol | WorkerEnvironment | Object-safe typed methods accepting WorkerEnvironmentContext and existing business requests; return existing typed results through AgentFuture/AgentResult |
| WorkerEnvironment / existing MCP owner | GetWorkspaceInfo / CompleteMcpAuthentication | Resolve target defaults without a Session; deliver a bound OAuth callback to the existing attempt, retaining existing WorkspaceInfo/McpAuthentication results |
| agent-client / talos-worker | Remote environment client / local implementation | Same port, authenticated RPC or local calls; no upper-layer local/remote branches |
| AgentNodeService / AgentNodeStore | Existing list/create/update/lease methods enhanced | Optional endpoint, name search/total, name-only update and shared revocation; no separate Computer Store |
| SessionStore / ProjectStore | Existing methods enhanced | Node-scoped list, project affinity and has_nonterminal_turns(nodeId); no new execution Store |
| SDK / Desktop host | computers.* / desktop/workerIdentity | Actor-facing product methods / safe local identity. No node credentials in renderer identity or list responses |
| AgentNodeService | lock_lifecycle / outbound_deployments / revoke_publication / mark_registration_published / withdraw_publication | Authenticate the current registration and advertised deployments; replacement and cleanup compare its current revocation hook so an old stream cannot withdraw a new one |
| AgentNodeStore | list(scope/query/cursor/limit), rename, set_status | Typed All/Owner/Embedded scope; count and page share a read transaction; partial mutations retain unrelated fields |
| agent-client | AgentTlsTrust::SystemRoots / Certificate | One trust input for node, resource and direct Harness clients; custom CA is optional, hostname verification remains enabled |

CreateNodeRequest.endpoint and UpdateNodeRequest.endpoint become optional across domain/PB/product transports.
An absent endpoint denotes outbound access; a present endpoint denotes the supported direct deployment.
The legacy endpoint field number is preserved with protobuf presence. Name-only edits use RenameComputerRequest,
not read-modify-write of the full UpdateNodeRequest. Domain owners and generated callers change together.

## Desktop host API

These named methods use the existing stdio App Server protocol and its SDK. They are not Agent business PB definitions.
The trusted host performs enrollment through AgentNodeClient's existing management gRPC service with Actor Bearer auth.
Configuration remains snake_case; host DTOs and their generated schema/TypeScript use camelCase.

| Method | Params | Response / caller expectations |
| --- | --- | --- |
| desktop/connection/read | null / omitted | DesktopConnectionResponse { connectionMode: cloud or local, controlPlaneEndpoint: string } |
| desktop/connection/set | DesktopConnectionSetParams { connectionMode } | DesktopConnectionResponse; main restarts the existing host after a durable save |
| desktop/workerIdentity | null / omitted | DesktopWorkerIdentityResponse { nodeId: string or null, controlPlaneEndpoint: string or null }; local returns embedded/null; not enrolled returns null/null |
| desktop/workerEnroll | DesktopWorkerEnrollParams { nodeId?: string or null } | DesktopWorkerIdentityResponse; null creates/reuses, an explicit existing UUID recovers by rotating its credential |

These bootstrap methods require the local host connection; enrollment additionally verifies its cached Cloud Auth login,
checks any supplied Actor credentials match that login, and confirms immutable owner/UUID through GetNode.
The UUID cannot be rebound by account change or hostname matching. Recovery cannot replace a saved UUID or control plane.
CreateNode/RotateCredential results stay inside the Rust host. Neither identity IPC nor runtime state contains node secrets.
`desktop/workerIdentity` is also exposed as a dedicated preload IPC method.

AgentNodeRunnerConfig.transport selects outbound/direct. A missing transport with existing listener/TLS fields preserves
released direct configs; a missing transport without listener fields selects outbound. Outbound forbids listener identity,
while direct still requires it. Cloud enrollment always persists explicit outbound. No new node table or RPC state is added.

Electron closes stdio through EOF and waits for host drain with the existing 240-second shutdown budget; forced termination
is reported as a shutdown failure. Worker enrollment failure is projected separately as ApiError on the existing runtime-state
notification and never changes a successful auth response into a logout. The Rust host observes terminal
Runner failures through its lifecycle callback and reports the original error once. The existing JSONL transport emits
`desktop/worker/failed` with an `ApiError`; Electron projects it onto runtime-state without clearing account identity.
The Desktop management page permits explicit recovery of an owned outbound node when first enrollment saved only
the pending marker. It passes the selected UUID to the existing host method; it never infers identity from names.
When stopping a Worker whose registration was rejected or expired, successful execution cleanup reports the original
registration error without blocking credential recovery or sign-out. The Desktop lifecycle handler consumes and reports
terminal Runner failures once through the host notification; execution cleanup and task join failures still propagate.
Without that handler, standalone wait retains the original Result and shutdown propagates unhandled non-auth registration failures. Cloud product calls use the cloud SDK;
the cloud host rejects local business dispatch and never creates a shadow product database.

## 2. Node control service

Extend AgentNodeControl in agent-pb/proto/node.proto:

```protobuf
service AgentNodeControl {
  rpc Register(RegisterRequest) returns (RegisterResponse);
  rpc RenewLease(RenewLeaseRequest) returns (RenewLeaseResponse);
  rpc Heartbeat(HeartbeatRequest) returns (HeartbeatResponse);
  rpc Unregister(UnregisterRequest) returns (UnregisterResponse);
  rpc Connect(stream NodeToControlFrame) returns (stream ControlToNodeFrame);
}
```

The first Worker frame is exactly one ConnectNodeRequest with the current lease. The control plane validates
node status, lease and ownership, installs advertised clients, then returns ConnectNodeResponse.ready.
Register records identity/lease/deployments but does not independently make an outbound Worker executable.
Heartbeats and lease renewal remain ordinary outbound gRPC calls; do not add a second heartbeat in Connect.
Registration credentials are only valid at Register, never at execution or resource ingress. Each finite RPC deadline
is enforced by both the control-side client and the Worker. A stopped or unreachable Worker cannot postpone a client
Timeout indefinitely; expiry cancels only that invocation through the existing call cancellation path.

The following transport messages live in a new agent-pb/proto/connection.proto; imports reuse auth.proto,
harness.proto, memory.proto and types.proto. Environment method fields are added when their canonical PB
projections are introduced in the environment slice; this sketch defines the full existing Harness coverage.

```protobuf
message ConnectNodeRequest { NodeAuth auth = 1; Empty params = 2; }
message ConnectNodeResponse {
  oneof result { Empty ready = 1; Failure failure = 2; }
}
message NodeToControlFrame {
  oneof body {
    ConnectNodeRequest connect = 1;
    WorkerRpcResult result = 2;
  }
}
message ControlToNodeFrame {
  oneof body {
    ConnectNodeResponse connected = 1;
    WorkerRpcCall call = 2;
    CancelWorkerRpcCall cancel = 3;
    AcknowledgeWorkerRpcResult acknowledge = 4;
  }
}
message WorkerRpcCall {
  uint64 call_id = 1;
  uint64 sequence = 2;
  bool requests_closed = 3;
  // Relative timeout on first frame; zero means no additional transport deadline.
  uint64 timeout_ms = 4;
  uint32 response_window = 5;
  oneof request {
    StartRequest start = 10;
    SubmitInputRequest submit_input = 11;
    InterruptRequest interrupt = 12;
    CompactSessionRequest compact_session = 13;
    SessionRequest clear_session = 14;
    SessionRequest close_session = 15;
    SessionRequest read_workspace_memory = 16;
    UpdateWorkspaceMemoryRequest update_workspace_memory = 17;
    FormDreamMemoryRequest form_dream_memory = 18;
    ShutdownRequest shutdown = 19;
  }
}
message WorkerRpcResult {
  uint64 call_id = 1;
  uint64 sequence = 2;
  bool completed = 3;
  oneof response {
    RunResponse run = 10;
    InputResponse input = 11;
    InterruptResponse interrupt = 12;
    EmptyResponse empty = 13;
    WorkspaceMemoryResponse workspace_memory = 14;
    FormDreamMemoryResponse dream_memory = 15;
    // RPC status/framing failure; method results retain their existing response shape.
    Failure failure = 16;
  }
}
message AcknowledgeWorkerRpcResult {
  NodeAuth auth = 1;
  uint64 call_id = 2;
  uint64 sequence = 3;
}
message CancelWorkerRpcCall {
  NodeAuth auth = 1;
  uint64 call_id = 2;
}
```

The first call frame grants an eight-message response_window. Worker dispatch acquires one per-call credit before
emitting each business response; the invocation consumer returns cumulative AcknowledgeWorkerRpcResult.sequence
on that call's ordered queue. The Worker verifies lease, monotonic acknowledgement and its sent high water;
closed calls ignore late acknowledgements. EOF/transport failure uses the reserved terminal slot without a credit.
Credits and counters exist only for the live invocation. They are not Timeline acknowledgements or durable commits.
A consumer that releases no window within five seconds fails only that call; other calls continue. This applies
backpressure to ordinary buffered model bursts instead of failing as soon as the first receive queue fills.
No business replay, unbounded delivery tasks or additional connection/channel is introduced.

First request frames carry strictly increasing positive call_id values within the live connection; request and response sequences start at one and increase per call.
They correlate in-memory channels only and are never DB keys. No method-name strings, arbitrary JSON, raw
serialized business bytes or durable RPC queue are introduced. Existing request objects already contain NodeAuth;
each request frame, including commit/model-reply frames, validates it against the active node connection/lease.
Failure projects the existing gRPC Status contract, including structured authentication/validation rejection;
method response bodies keep their original result/failure variants. Generated Rust boxes only the new carrier
oneof payloads to bound envelope size; the existing business types and protobuf wire shape do not change.

requests_closed is the per-call request-stream half-close; completed is the per-call response EOF, not a new
business settlement. Unary calls close requests with their first frame. Start/SubmitInput/Compact preserve
admission then exactly one commit, accepted then existing results. Dream formation preserves its callback frames.
CancelWorkerRpcCall closes a pending invocation using the existing permit/stream-drop semantics; it does not
replace the business Interrupt RPC or claim that a committed task was successfully cancelled.

## 3. Resource callbacks and execution invariants

| Direction | Existing service | Required behavior |
| --- | --- | --- |
| Control plane -> Worker | AgentHarnessRpc request/response messages through Connect | Reuse the existing adapter validation, admission permit and event/settlement conversion |
| Worker -> control plane | AgentSessionDataRpc | Session history, attachment chunks and memory progress; authorize the stored Session/node/owner |
| Worker -> control plane | AgentSubagentControlRpc | Spawn/send/wait/interrupt/follow-up/list with the existing typed Agent caller/target objects |
| Worker -> control plane | AgentToolImageRpc | Existing streamed upload and FileService metadata |
| Control plane -> client | Existing Timeline HTTP/stdio stream | Persist through the existing Timeline writer; clients replay durable product events using existing cursors |

Resources remain normal outbound calls to the same control plane. They do not need another reverse transport,
desktop relay or new business protocol. Frames never rename existing RunParams, Admission, AgentEvent,
Settlement, continuation or model callback objects into duplicate remote-only business types.

Use bounded per-call channels and frame/upload limits derived from existing transport limits. Do not buffer
unbounded events or hold the connection reader while executing a tool or waiting for a resource callback.
Cancellation, relative deadlines and structured failures propagate to the existing handling owner.

The connection reader uses nonblocking per-call dispatch; it never awaits capacity in a full business queue.
A full queue ends only that call with the existing ServiceUnavailable error, fails its waiting consumers and
requests per-call cancellation. Uncommitted admission is released; committed work follows the existing
interrupt/disconnected cleanup and unconfirmed-outcome rules. Never drop events and then report success.

The single writer reserves bounded capacity for handshake, cancellation, failure/EOF, Interrupt and
admission/commit/callback control frames, and fairly schedules data across calls. Priority cannot overtake
earlier frames within the same call, including progress before settlement. Do not create another network
channel, durable queue or scheduler service. If control delivery itself cannot proceed within its deadline
or capacity, fail the connection explicitly and apply the existing connection-loss cleanup; do not hang.
Admission and queues use agent-client transport limits: 32 ordinary active calls plus 4 reserved control
calls (Interrupt/Shutdown/SubmitInput), 8 frames per call, a 44-frame control queue, and a 5-second writer
delivery limit. The first request and cancellation use reserved control capacity; subsequent commit/callback
frames use their call's ordered queue. Each result queue reserves a terminal slot so failure/EOF cannot
overtake prior progress. StreamMap schedules available call queues fairly. Normal readers yield between
buffered frames so a healthy consumer is not misclassified as stalled. Unary requests require no request pump.
Existing tonic frame limits and ordinary resource-upload limits remain unchanged. Late frames
for ended calls are rejected/discarded as cancelled transport delivery, never accepted as a fresh invocation.
Validate with one stalled consumer plus another call's Interrupt/commit/Dream reply, as well as preserved
per-call ordering and confirmed versus unconfirmed interruption. No extra error enum is introduced.

Connection loss withdraws execution availability immediately and stops new admission on both sides. Pending
uncommitted permits are released; committed work enters existing cancellation/failed-stream handling and
must not be reported as confirmed cancellation or success without its actual outcome. An uncertain file/tool
effect is not undone or replayed. Surface ServiceUnavailable/Timeout through the existing error contract.
The Worker does not start new cloud work until the old invocation is locally released and its connection is valid.
Reconnect restores registration, not live RPC calls: no automatic replay, new provider checkpoint scheme,
exactly-once claim or transport event persistence. Existing Turn recovery/continuation rules remain authoritative.
Adapter cleanup that returns an error or CleanupUnconfirmed blocks automatic re-registration for that runtime;
the host must recover explicitly. It is a live cleanup result, not a new persisted node/RPC state.
Server shutdown closes its live Connect streams before waiting for gRPC graceful drain. Bare proxy/HTTP2
Unknown/Internal/Unavailable failures map to ServiceUnavailable at SDK ingress; structured business errors
retain their original codes. Registration retry never means retrying a submitted operation.

Reject frames from superseded connections even when the node UUID is unchanged. Compare the active connection
handle before publication, callback dispatch and unpublication; do not persist a connection generation or ID.
Disabling a node or rotating credentials closes the stream and uses the same revocation path for direct clients.

## 4. Worker environment PB and configuration boundary

WorkerEnvironmentRpc exposes the capability methods listed in the parent design, each with a concrete request
and response. The outbound carrier adds one typed request/response case for each method; it does not invoke a
generic Execute(JSON) operation. Direct and outbound carriers dispatch to the same WorkerEnvironment owner.

The stage 3.1 source contract is [environment.proto](../../talos-rs/crates/orchestration/agent-pb/proto/environment.proto).
The canonical Rust port is [WorkerEnvironment](../../talos-rs/crates/orchestration/agent-protocol/src/environment.rs);
[AgentEnvironmentClient](../../talos-rs/crates/orchestration/agent-client/src/environment.rs) hides direct/Connect transport.
TalosWorker composes WorkerEnvironmentService from its live configuration and node-local filesystem owners.
The control-plane TalosService does not assemble a directory filesystem service.

| gRPC method | Business parameters | Successful result |
| --- | --- | --- |
| GetConfig / UpdateConfig / DeleteConfig / ReloadConfig | Existing Config*Request; optional product nodeId is consumed before RPC | Existing ConfigResponse / ConfigDeleteResponse / ConfigReloadResponse |
| GetWorkspaceInfo | WorkspaceInfoParams (empty); optional saved Workspace in context | Canonical WorkspaceInfoResponse projection |
| ListWorkerModels | ModelListParams (empty) | Existing ModelListResponse catalog and default selection |
| ReadDirectory | Existing FsDirectoryReadRequest path | Existing FsDirectorySnapshot |
| CreateDirectory | Existing FsDirectoryCreateRequest parent/name | Existing FsDirectoryCreateResponse path |
| GetProjectStatus | Project directory path | Existing ProjectStatus |
| ValidateProjectDirectory | Project directory path | Worker-canonical absolute directory |
| ListWorkspaceDocs / ReadWorkspaceDoc / WriteWorkspaceDoc | Existing WorkspaceDoc*Request fields, including kind/content/expected_version | Existing WorkspaceDoc*Response with the same document/version objects |
| ListMemoryFiles / ReadMemoryFile / WriteMemoryFile | Existing MemoryFile*Request fields, including dates/content/expected_version | Existing MemoryFile*Response; writes update the Worker's existing index |
| SearchMemory / GetMemory | Existing MemorySearchRequest filters / MemoryGetRequest.memory_id | Existing MemorySearchResponse / MemoryGetResponse |
| ListSkills / RefreshSkills / SuggestSkills / ReadSkill / ResolveSkills | Existing Skills*Request fields and SkillSelector | Existing Skills*Response, preserving summaries, metadata, load errors and warnings |

Stage 3.2 capability sources are [environment_memory.proto](../../talos-rs/crates/orchestration/agent-pb/proto/environment_memory.proto) and [environment_skills.proto](../../talos-rs/crates/orchestration/agent-pb/proto/environment_skills.proto); document projections and service envelopes stay in environment.proto.
Enums reject unknown values; optional Memory kind filters preserve absent versus present-empty, and counts use checked integer conversions.

Stage 3.3.1 uses [environment_config.proto](../../talos-rs/crates/orchestration/agent-pb/proto/environment_config.proto). Provider model metadata reuses the existing ModelInfo projection. HTTP config routes accept nodeId in the query and preserve their existing business bodies; JSON-RPC consumes it from the canonical request. POST /v1/config/reload provides the same explicit reload. Worker ConfigService has no SessionStore; the control plane queries existing active Turns by their Session node under a node admission gate. Actual Harness execution holds the Worker gate through settlement. The Worker also publishes the validated Kernel execution configuration to the existing Provider; cached Sessions share that owner and consume new configuration on subsequent invocations. Changing the enabled deployment ID set requires Worker restart and registration, and is rejected before runtime publication. Config commits fail Conflict while that gate is in use. The adapter and Harness resolve the live Worker configuration; stale startup snapshots cannot reject a newly reloaded Workspace. Memory tools and Dream formation retain the same live Worker configuration owner and MemoryStore.
Each operation captures one configuration snapshot, including the Workspace timezone and Memory directory. Project
status reads project metadata even when a Worker RPC expires, returning nodeUnavailable; authorization and business
validation failures keep their original error codes.

Every request contains NodeAuth, WorkerEnvironmentContext and concrete parameters. Every response contains
one typed success or Failure. Connect carries a concrete typed request/response case for each method; it dispatches to the same adapter.
The context contains trusted user_id, optional username and optional workspace_id; it never fabricates a Session.
Product nodeId is consumed by Service routing; an environment client is already bound to that node and does not
serialize another routing target into the business parameter projection. Empty nodeId is invalid.
Local calls use the same context without NodeAuth. The adapter checks lease and owner before invoking the owner.
Document/Memory/Skills product requests add optional nodeId; absent/null selects embedded, an explicit node is authorized and must be available. Service consumes nodeId before calling the port. HTTP path-based document/Memory reads and document writes carry nodeId in the query; Memory writes and Skills read/resolve carry it in their existing request body. SDK convenience methods accept an optional target without changing existing calls.
Version tokens, MEMORY.md canonical validation, Memory index replacement and Skills semantics remain with their existing Worker owners. Live Worker configuration resolves defaults; no files, indexes or installation managers are recreated on the control plane.

Directory fields, order, errors and path policy reuse the original filesystem DTOs. Stored directory metadata
is not canonicalized or checked by the control plane; those operations happen on the selected Worker.

WorkspaceInfoResponse retains its existing camelCase JSON fields. Project's existing JSON fields retain their
wire spelling; nodeId is the added camelCase field. Omitted/null nodeId keeps existing selection semantics:
embedded for directory/environment/Project creation, all owned nodes for lists, existing Coordinator policy for
new Sessions without a Project, and the immutable binding for existing Sessions. Project selects its saved node;
an explicit conflicting target fails InvalidRequest. Router selection must be deployed on that target.
Project metadata remains readable offline with the typed nodeUnavailable status. Other RPC failures propagate.

An omitted workspace_id selects the target Worker's configured default; an explicit empty ID is invalid.
GetWorkspaceInfo bootstraps with omitted workspace_id and empty WorkspaceInfoParams, and returns
the canonical WorkspaceInfoResponse. It requires Actor/lease ownership, not a pre-known Workspace ID.
The control plane calls it after resolving a new Session's node, stores the returned nonempty workspace_id,
and uses that stored ID for subsequent Harness requests. Existing SessionContext.workspace_id stays required.
Environment calls may resolve the target default without a Session; explicit IDs must exist on that Worker.
Never copy root/configuration into the control-plane resolver or rewrite existing bindings after default changes.

Configuration updates hold control-plane admission exclusion for the target node across policy checks and
the remote operation. The Store checks only that node's nonterminal Turns; all start/input/queue/continuation
paths honor that gate. Worker-side exclusion protects actual execution, candidate construction, durable file
commit and runtime publication. Locks must not block resource callback reads or another node's tasks.
Secrets stay on the Worker and use existing redacted responses. Failure does not trigger write retries.

## 5. Remote MCP OAuth callback

Extend the existing OAuth owner and attempt lifecycle, without introducing a second OAuth manager or Store.
The existing McpAuthenticationStartRequest/Response and ReadMcpAuthentication contracts remain authoritative.
StartMcpAuthentication's internal RPC envelope adds a typed callback target: Loopback for explicit local mode,
or ControlPlaneRelay with the configured control-plane HTTPS callback URI. This is adapter input, not a
user-supplied redirect URI in the public business request. The Worker verifies the relay origin/path against
its configured control plane, and uses that exact URI for authorization and token exchange.

| Boundary | Contract |
| --- | --- |
| Product StartMcpAuthentication | Authorize Actor/node/McpDefinitionRef; invoke target Worker and return the existing attemptId/authorizationUrl/expiresAt only after callback routing is ready |
| HTTP GET /v1/mcp/oauth/callback | Parse standard state, code or provider error, and issuer; resolve the exact pending route, forward to its original Worker attempt; return bounded completion/error presentation without echoing callback secrets |
| Worker CompleteMcpAuthentication | NodeAuth + trusted WorkerEnvironmentContext + McpAuthenticationCallbackParams -> existing McpAuthenticationResponse or Failure |
| Product ReadMcpAuthentication | Existing server reference plus node target -> existing McpAuthenticationState; transport delivery is not proof of authentication |

McpAuthenticationCallbackParams contains the existing server reference, attempt_id, state, optional issuer,
and exactly one typed outcome: authorization code or provider denial. It never carries user IDs, PKCE verifier,
client secrets or access/refresh tokens. These are new callback delivery fields, not duplicated authentication
states. Add the concrete CompleteMcpAuthentication request/response case to the outbound environment carrier.

```protobuf
message McpOAuthCallbackTarget {
  oneof target { Empty loopback = 1; string control_plane_relay_uri = 2; }
}
message StartMcpAuthenticationRequest {
  NodeAuth auth = 1;
  WorkerEnvironmentContext context = 2;
  McpAuthenticationStartParams params = 3;
  McpOAuthCallbackTarget callback_target = 4;
}
message McpAuthenticationCallbackParams {
  McpDefinitionRef server = 1;
  string attempt_id = 2;
  string state = 3;
  optional string issuer = 4;
  oneof outcome {
    string authorization_code = 5;
    // Standard OAuth error identifier, not an error description or display text.
    string provider_error_code = 6;
  }
}
message CompleteMcpAuthenticationRequest {
  NodeAuth auth = 1;
  WorkerEnvironmentContext context = 2;
  McpAuthenticationCallbackParams params = 3;
}
message CompleteMcpAuthenticationResponse {
  oneof result { McpAuthenticationResult value = 1; Failure failure = 2; }
}
```

McpDefinitionRef and McpAuthentication* PB messages project the existing canonical product objects.
The Worker reconstructs the configured callback URL from these standard fields and delegates OAuth
validation/exchange to the existing SDK. Malformed or mutually exclusive outcomes fail before consumption.
The callback response uses no-store/no-referrer policies and its access logs omit query parameters.

Before publishing the authorization URL, the control plane parses its state according to OAuth URL syntax
and attaches a bounded, expiring route to the existing node publication owner:
state_hash -> { actor_user_id, node_id, server, attempt_id, expires_at, active_connection }.
Use the existing ten-minute attempt deadline and pending-attempt capacity as the bounds. The entry contains
no PKCE/token and is not persisted. State is not rewritten or accepted as a user identity. For shared embedded,
the route retains the authorized initiating Actor; callbacks cannot acquire administrator rights themselves.

The callback requires no browser bearer header: the recorded route supplies the trusted Actor/target, and
the original Worker SDK verifies state and issuer. Only one callback per attempt is processed concurrently;
the Worker remains the authority for one-time state consumption, token exchange and durable credential commit.
Invalid state/issuer or pre-consumption rejection must not consume a valid pending attempt. Successful or
terminal/expired attempts remove the route; a repeated completed callback cannot exchange again. Do not
branch on provider error text or log state/code/authorization URLs/secret-bearing parent requests.

Node revocation, superseded connection, logout, definition replacement, attempt expiry or control-plane
restart invalidates routing; the callback returns ResourceNotFound/ResourceExpired/ServiceUnavailable as
appropriate without node or credential disclosure. A transport failure never replays an authorization code.
Use ReadMcpAuthentication to inspect an uncertain outcome; a new start uses the existing cancellation of
the old definition attempt. Routing allocation failure does not publish an unusable URL; invalidate that
attempt through the existing attempt lifecycle. Plain local OAuth keeps its loopback behavior.
