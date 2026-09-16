# Turn Task Lifecycle Control

Status: archived
Owner: agent
Updated: 2026-08-22

## 1. Goal

Each active `(session_id, turn_id)` has one settlement owner. Every Run-owned asynchronous task, including Runtime, Tool, and MCP request work, must be registered before execution and reach a confirmed or explicitly unconfirmed terminal state before the Run settles.

Timeline subscriptions do not own the Run. Disconnecting, switching Sessions, or resubscribing must not cancel server-side execution. Only an explicit interrupt/stop changes product cancellation state. Harness may also use cancellation settlement for internal teardown or activation-failure cleanup, but it does not publish that path as a user cancellation.

## 2. Ownership

The independent `talos-run-control` crate owns the lifecycle state machine and has no dependency on Agent, Kernel, Tool, Service, or transport modules.

```text
Agent Harness
  └── ActiveRunControl
        └── RunTaskGroup
              └── RunTask<T>
```

- `talos-run-control` owns the lifecycle implementation.
- Harness owns the `ActiveRunControl` instance and is the only settlement caller.
- Runtime and Kernel receive a cloned `RunTaskGroup` capability.
- The `RunTaskGroup` instance directly owns every registered task handle and cancellation signal.
- Each task owns cleanup of resources it creates.
- A shared MCP connection remains application-owned; only the Run-owned MCP request task is cancelled.
- Service persists product state and never owns Tokio tasks.

## 3. Public API

```rust
ActiveRunControl::new(identity)
ActiveRunControl::tasks() -> RunTaskGroup
ActiveRunControl::settle(termination: RunTermination) -> Future<Output = Settlement>

RunTaskGroup::spawn(
    factory: FnOnce(Cancellation) -> Future<Output = RunTaskCompletion<T>>
) -> Result<RunTask<T>, SpawnError>
RunTaskCompletion::Confirmed(output: T)
RunTaskCompletion::CleanupUnconfirmed(output: T)
Cancellation::requested() -> Future<Output = ()>
RunTask<T>: Future<Output = Result<T, RunTaskError>>
```

`spawn()` supplies the shared `Cancellation` observer to the task factory. Callers cannot retrieve or trigger the cancellation signal separately.

`RunTermination` is exhaustive and strong typed:

```rust
struct RunIdentity {
    session_id: String,
    turn_id: String,
}

enum RunTermination {
    Completed { timeout: Duration },
    Cancelled { policy: CancellationPolicy },
}

struct CancellationPolicy {
    grace: Duration,
}

enum RunTaskCompletion<T> {
    Confirmed(T),
    CleanupUnconfirmed(T),
}

enum CancellationMode { Cooperative, Forced }
enum SettlementKind { Completed, Cancelled(CancellationMode) }
enum CleanupFailure {
    OwnerUnconfirmed,
    DeadlineExceeded,
    ForceAborted,
    TaskDropped,
}
enum CleanupReport {
    Confirmed,
    Unconfirmed { count: usize, failures: Vec<CleanupFailure> },
}
struct Settlement {
    kind: SettlementKind,
    cleanup: CleanupReport,
}
enum SpawnError { Closed, CapacityExceeded }
enum RunTaskError { Stopped }
```

A task returns `RunTaskCompletion::Confirmed(output)` or `RunTaskCompletion::CleanupUnconfirmed(output)`. The latter always records `OwnerUnconfirmed`; task owners cannot select framework-generated failure variants. `RunTask<T>` exposes only the business output; cleanup is aggregated into `Settlement`.

Settlement records `DeadlineExceeded` when it aborts an unfinished task after a `Completed` timeout and `ForceAborted` after a `Cancelled` grace deadline. `TaskDropped` is reserved for a task future that exits unexpectedly before settlement owns and aborts it, such as panic or Tokio runtime shutdown.

The group accepts at most 256 active tasks. A full group returns `SpawnError::CapacityExceeded`; a settling or settled group returns `SpawnError::Closed` before starting the factory.

There are no Tool-, MCP-, or Runtime-specific spawn methods. There is no public `close`, cancellation request, abort, cleanup, task handle, token, channel, or bookkeeping API. No compatibility aliases are retained.

## 4. State Machine

```text
Running
  └── settle(Completed | Cancelled)
        └── Settling
              └── Settled
```

The first `settle()` call atomically owns the terminal transition. Concurrent or repeated calls wait for and return the same immutable `Settlement`.

One internal settlement path performs, in order:

1. Reject new task registration.
2. Broadcast cancellation only for `Cancelled`.
3. Wait for registered tasks within the completion timeout or cancellation grace deadline.
4. Abort remaining tasks after the deadline.
5. Aggregate cleanup and publish one settlement.

Callers cannot partially execute or reorder these steps.

## 5. Task Contract

- Registration succeeds before the task factory starts creating resources.
- All task kinds use the same `spawn()` behavior, capacity limit, cancellation signal, cleanup contract, and deadline policy.
- Normal task completion reports `Confirmed` unless the task owner explicitly reports `OwnerUnconfirmed` through `RunTaskCompletion::CleanupUnconfirmed(output)`.
- A task future that exits without returning `RunTaskCompletion<T>` produces typed unconfirmed cleanup and is never reported as confirmed. Dropping the caller's `RunTask<T>` does not drop the underlying task.
- Business task type is not used as lifecycle policy or telemetry classification.
- MCP request identity and process handles remain inside their owning Tool implementation.

A Run-owned task is asynchronous work whose cleanup can block settlement because it owns or coordinates Runtime, Tool, process, or request resources. A Harness controller that only awaits group-owned tasks or forwards bounded events is not registered in its own group; it cannot own business resources or outlive `ActiveRunControl`.

## 6. Product Semantics

- `RunTermination::Completed` settles without broadcasting cancellation.
- Confirmed normal completion preserves the successful terminal event produced by Runtime.
- An explicit interrupt with cooperative confirmed cleanup produces a cancelled Turn terminal state.
- An explicit interrupt whose cancellation grace expires produces `SettlementKind::Cancelled(Forced)`, `CancelActiveTurnOutcome::ForceAborted`, and `RunForcedAbort`, regardless of other cleanup failures in the same report.
- `OwnerUnconfirmed`, `DeadlineExceeded`, or `TaskDropped` produces `RunCleanupUnconfirmed` for every non-forced settlement, and must not restore the Turn to `Running`.
- Service interrupt/stop routes to Harness, which calls `settle(RunTermination::Cancelled { policy })`; no lower layer may request cancellation directly.
- Harness-owned teardown and activation-failure cleanup may use the same cancellation settlement for resource cleanup without publishing a user cancellation result.
- Existing multi-client incremental subscription and recovery behavior remains unchanged.
- Cancelling `WaitingUserAction` persists the action as cancelled, then persists the Tool item as cancelled, then persists the cancelled Turn terminal event. It does not call a missing Runtime task. Session-scoped write serialization prevents the waiting handoff from replacing this terminal path.

## 7. Observability

Structured tracing includes `session_id` and `turn_id`:

- `run_task_registered`: `task_id`, `active_tasks`
- `run_cancellation_requested`: `active_tasks`, `grace_ms`
- `run_settlement_started`: `termination`, `active_tasks`
- `run_force_abort_started`: `termination`, `active_tasks`, `elapsed_ms`
- `run_task_cleanup_completed`: `task_id`, `cleanup_outcome`, `active_tasks`, `duration_ms`
- `run_settled`: `settlement_kind`, `cleanup_outcome`, `unconfirmed_tasks`, `duration_ms`

`active_tasks` and `unconfirmed_tasks` are counts. For `run_task_cleanup_completed`, `duration_ms` starts at task registration; for `run_settled`, it starts at `ActiveRunControl` creation. `elapsed_ms` is the settlement deadline that elapsed before abort. Task business type is logged by its owning Runtime or Tool layer, not by `talos-run-control`. This change adds no per-Run Metric labels: IDs remain tracing fields only. Logs must not contain Tool arguments, outputs, environment variables, or other sensitive data.

`cleanup_outcome` is `Confirmed` only when the report contains no failure; any failure produces `Unconfirmed`. `run_settlement_started` samples `active_tasks` after registration closes and cancellation is broadcast when requested. `run_force_abort_started` samples it after the deadline and before remaining tasks are drained.

The tracing events provide counter and duration inputs for settlement count, force-abort count, unconfirmed cleanup count, and settlement latency. Metric dimensions are limited to `termination`, `settlement_kind`, and `cleanup_outcome`; `session_id`, `turn_id`, and `task_id` remain tracing-only correlation fields.

## 8. Acceptance

- Runtime, ordinary Tool, and MCP request execution all use `RunTaskGroup::spawn()`.
- No production path creates a Run-owned asynchronous task outside the group. Blocking resources must be owned by an asynchronous supervisor that can release or explicitly report them as unconfirmed; Tokio cannot force-stop arbitrary blocking code.
- Explicit cancellation settles all registered tasks cooperatively or force-aborts them within the policy.
- Normal completion rejects new tasks and waits for existing tasks through the same settlement entry point.
- Subscription disconnect and Session switching do not cancel the Run.
- Parallel task and supervised external-process cancellation produces one deterministic settlement and cleanup traces.
