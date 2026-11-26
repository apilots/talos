# Embedded Worker library calls

Issue: https://github.com/apilots/talos/issues/206

Status: implemented

## Background and current facts
Desktop/TUI application composition builds one TalosWorker, then starts a loopback WorkerRpcHost and registers RemoteAgentClient handles. History, memory and tool callbacks are replaced with a second loopback resource client. The raw AgentHarness typed SPI already exists, but SessionGate, SessionOwner, auth, resource binding and settlement fencing currently live in AgentHarnessRpcAdapter. Registering raw harnesses would bypass those checks and is not acceptable.

## Goals and decisions
Reuse the existing typed AgentHarness and WorkerEnvironment ports. Expose the existing adapter's execution boundary in-process, sharing its gates and lifecycle with remote RPC; keep network encoding only at the external adapter. Bind original product resource ports for local calls rather than serializing history through loopback. Preserve the authenticated local node identity on the history capability while forwarding reads and supplements to its original owner. RPC preflight gate rejection remains Status, distinct from Harness-operation failure payloads. No separate runtime, manager, API framework or code module. Keep changes inside existing owner files; their existing size is preferable to inventing a parallel module under this task's explicit no-new-modules constraint.

## Non-goals
No persistence/authority migration, Sync changes, WebSocket work, PR205 implementation, container desktop direct connection or remote gRPC removal.

## Impacted areas
Existing agent RPC admission/settlement owners, Worker host/resource composition, application runtime, focused existing test files and reusable architecture comments.

## Verification strategy
Focused nextest, check/clippy/fmt/repository policy checks. Run a representative embedded task with large history (approximately 8MB), typed admission/continuation/cancel/shutdown and existing real gRPC compatibility cases using supported fixtures. Report missing real Desktop GUI/model/other-topology prerequisites separately; do not claim this fixes Sync uploads.

## Risks and followups
Local and remote handles must share the same causal and ownership gate. Preserve two-phase permit consumption and drop/lease cancellation cleanup, including AskUserQuestion waiting. Avoid replay. Worker transport task monitoring must not treat a deliberately absent local listener as failure.
