# Worker outbound WebSocket

Status: implemented
Issue: https://github.com/apilots/talos/issues/207

## Background and goals
Personal Desktop, TUI and Native Workers cannot accept inbound connections.
Use authenticated outbound WSS for node registration/lease operations and the
bidirectional task/event carrier. Keep cloud Docker Workers on gRPC explicitly.
Reuse existing modules, typed protobuf messages and lifecycle owners.

## Non-goals
No persistence/authority/Sync changes, new crates or code modules, relay service,
replay protocol, SDK fork, desktop media redesign or embedded invocation changes.
The companion task owns embedded Library-direct calls.

## Current implementation and reuse map
- `agent-protocol::AgentHarness` / `WorkerEnvironment`: typed execution API.
- `talos-agent::AgentHarnessRpcAdapter::dispatch_connect`: current typed adapter;
  preserves per-request authorization, Session admission and cleanup.
- `agent-client::AgentNodeRunner`: registration, lease renewal, retry, fencing,
  disconnect drain and terminal error owner; no execution replay.
- `agent-client::worker_connection`: Worker dispatch/correlation/response windows.
- `agent-client::AgentExecutionConnection`: Server pending calls, cancellation,
  heartbeat and bounded writer; reused unchanged by both carriers.
- `talos-base::NodeControlRpc`: node authentication and registration owner.
  `agent_node_connect` publishes the existing connection into Coordinator.
- `talos-worker::RegisteredWorker`: common Desktop/TUI/Native registration host.
- `talos-service::agent_node::managed_worker`: cloud deployment seed owner.

## Design decisions
1. Outbound personal registration selects WSS, not a gRPC fallback. HTTPS endpoint
   configuration derives WSS URLs on the same TLS listener. Existing Actor-backed
   enrollment/management and resource APIs are unchanged.
2. Existing protobuf node lifecycle requests/responses are binary messages on
   short WSS exchanges at method-specific paths. The existing Connect envelopes
   carry all tasks/events over one long-lived outbound WSS connection. No generic
   JSON-RPC framework, second registry or replay is introduced.
3. Share TLS identity and port with HTTP/2 gRPC using maintained HTTP server and
   WebSocket libraries. Preserve certificate/hostname validation. Never downgrade
   to plaintext or fall back after a WebSocket failure.
4. Server Connect accepts a typed Stream rather than tonic-specific Streaming.
   WSS decodes into it; gRPC keeps using it. All authorization, publication,
   revocation, pending calls and cleanup remain in existing owners.
5. Cloud managed deployment explicitly selects outbound gRPC. Existing direct
   ingress remains for central Server self-publication, not personal computers.
6. Bound binary messages to the existing gRPC default envelope limit (4 MiB),
   retaining existing per-call flow control. WSS does not fix Sync upload sizes.

## Impact and compatibility
Server and personal clients must upgrade together; reverse proxies must forward
HTTP/1.1 Upgrade for node WSS paths while retaining HTTP/2 gRPC. Managed Docker
configuration explicitly chooses gRPC, without transport-error fallback. The
existing official-container packaging marker also preserves gRPC for persisted
cloud homes written before this transport choice existed.
Native hosts retain their independently configured local loopback API until the
companion embedded-library task removes unnecessary local RPC; no public Worker
listener or Server callback is needed for outbound execution.

## Deployment checklist
- Personal Worker configuration keeps `control_plane_endpoint: https://host:port`;
  omitted `transport` or `transport: outbound` selects WSS with the same CA trust.
  The client derives `wss://host:port/talos/worker/connect`. It does not bind a
  public execution listener, advertise an inbound endpoint or permit Server dialback.
- Official managed Docker seeds keep writing `transport: outbound` so a
  Server-first upgrade can provision a fresh home against the already-pinned
  Worker image. The existing `TALOS_WORKER_PACKAGING=official-container` marker
  makes the new Worker use gRPC, and the old Worker already treats `outbound`
  as gRPC. Explicit `outboundGrpc` remains for operator-managed configs. The
  Server retains the original `AgentNodeControl.Connect` HTTP/2 gRPC method.
  This is explicit deployment selection, not failure-triggered fallback.
- Central Server self-publication retains the existing direct TLS ingress. This
  does not select a personal Worker's outbound carrier.
- Forward `/talos/worker/*` HTTP/1.1 WebSocket Upgrade without buffering. Lifecycle
  paths are `register`, `renew_lease`, `heartbeat`, `unregister`; each exchanges the
  corresponding existing protobuf request/response. Task/event `connect` uses
  existing `NodeToControlFrame` / `ControlToNodeFrame` binary messages.
- Allow long-lived connections with the existing 15-second in-band heartbeat.
  Preserve TLS verification and HTTP/2 routing on the same port. No new public
  listener, plaintext option or automatic gRPC fallback is introduced.
- Deploy the Server before personal clients. Existing cloud gRPC clients remain
  supported; rolling back a personal client requires compatible transport config.

## Verification strategy
Exercise authenticated real TLS registration and task/progress/result using
existing fixtures for both carriers. Cover rejected credentials, revocation,
shutdown/disconnect without replay, cancellation, routing and envelope bounds.
Run scoped fmt/check/nextest/clippy and repository gates. Real multi-machine,
GUI and private-service acceptance are separately reported, never inferred.

## Risks
Resource callbacks and Actor enrollment continue to use their existing API;
this task does not switch all management/media APIs to WebSocket. Preserve
explicit typed transport errors rather than hiding a carrier failure.
