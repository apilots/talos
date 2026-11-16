# Session ownership

Status: active

Related request: [Issue #152](https://github.com/apilots/talos/issues/152)

Investigation baseline: `b2f23a6b0d526fe392a5f94fee736b511631992e`

## Problem

Several processes can open one `TALOS_HOME`. Nothing records which process owns a session, so the runtime cannot tell a live owner from a dead one.

- Every runtime starts `session-loop-scan`. A short-lived `talos exec` can claim another session's due Loop, exit, and leave that Loop `active` with `next_due_at = NULL`.
- After `kill -9`, the worker marker stays `cleanup_confirmed = false`. Later opens get `RunCleanupUnconfirmed` or `ActiveTurnRunning` forever.
- A second live instance does not kill the running turn, but it reports that same ambiguous error and retries about once a second.
- `talos exec` has no caller-supplied idempotency key, and exit 3 does not identify the waiting turn.

## Decisions

A Session stays bound to its owner account and original execution Worker. A message submitted from another machine still executes on the bound Worker in the saved Workspace. Recovery may change which process coordinates execution, while the owner, Worker, and Workspace bindings are preserved. An unavailable Worker leaves the Session unavailable for execution until that Worker returns.

1. **The worker agent owns the lock.** `SessionOwner` lives in `talos-agent`, beside the session gate that admits runs (`orchestration/agent`). `TalosService` calls that object; it does not implement a second lock. TUI, CLI, Web, and Desktop only translate `SessionHeld`.
2. **One OS lock per session.** Non-blocking exclusive `talos_fs::try_file_lock` at `{data_dir}/worker/session-locks/{sha256(session_id)}.lock`, next to `worker/session-heads`. The lock arbitrates processes using the same Worker data directory on one machine. The kernel drops the lock when the process dies. No heartbeat and no TTL. Lock acquisition is the only source of truth for this local process claim.
3. **File contents are for the error text.** After a successful lock, write `{ownerKind, pid, host, since}`. `ownerKind` is `tui`, `exec`, `cli`, `server`, or `desktop`. A reader that cannot parse the file still returns `SessionHeld`.
4. **One in-process table.** The embedded runtime builds one `SessionOwner` and shares it with the worker RPC adapter and the service. Each admission or maintenance operation takes a lease on that table. The Worker retains the exact consumed turn until its Local history commit. Service retains its operation lease through product settlement. Finishing an older turn releases only that turn's ownership. Another process gets `SessionHeld` and does not take over or signal the holder.
5. **Hold scope is a runtime policy**, set when the process starts:

| Process | Policy |
| --- | --- |
| TUI | `Attached`: one open session until switch or exit |
| `talos exec`, `goal start`, `goal resume`, `loop start` | `Process`: hold until process exit |
| Other CLI subcommands | `Turn` |
| `talos-server`, Desktop, SDK embed | `Turn`: admission through settlement |

Independent TUI and foreground CLI processes can contend for the same Session while it is idle, so their claims outlive a single Turn. In the usual single-backend deployment, Web clients and Desktop windows share their backend process. The backend acquires the Session lock for Turn admission and holds it through settlement. Opening a view or reading history does not reserve the Session.

6. **Open versus read.** History reads do not take the lock. `recover_timeline` claims the open session for `Attached` and `Process`. A failed claim returns `SessionHeld`, loads no recovery, and does not settle. TUI keeps the previous owned session, shows the target history as read-only, prints who holds it, and does not retry the subscription. Web and Desktop stop the event reconnect loop on `SessionHeld`. A deliberate send after this response goes through normal Turn admission so the service can check the current owner. HTTP maps the code to 409. `RunCleanupUnconfirmed` remains only for cleanup that this process still has in flight.
7. **Switching under a live turn.** `Attached` does not drop a session lock while this process still has a live turn there. The new session is not taken. After that turn settles, a later open can move the lock.
8. **Dead owner.** Under the Worker session lock, recovery reloads the durable marker and verifies the exact Turn, Agent, and coordinator. A local reservation or locally consumed Turn with unconfirmed cleanup remains unavailable for recovery. Otherwise the Worker records confirmed cleanup before Service settles the orphan and commits the history cursor. Recoverable waiting continuations resume from their durable state. A recovered non-resumable running turn settles `Failed` with the public reason `owner process exited`. Acquiring the session again after another process wrote it uses the latest marker.
9. **Start consults recovery before `ActiveTurnRunning`.** Start acquires ownership before changing session metadata or attempting recovery. Recovery completes before entering the automation admission gate. The lease stays live through new admission and settlement. A foreground `Process` hold remains until exit.
10. **Loop and Goal scan.** Resident Server, Desktop, and embedded SDK runtimes can claim free sessions with active automation. Foreground `loop start`, `goal start`, and `goal resume` drive their bound session. TUI drives its attached session. One-shot CLI commands, including `exec`, do not register the background scan. The scan takes a session lease before recovering an orphan or claiming a deadline. A held session is skipped before the scan writes. Explicit Goal Pause and Loop Stop can update the saved definition from a second CLI while the original owner completes its turn.
11. **Missed Loop occurrence.** A model decision accepted for a running Loop is durable and takes effect only after normal terminal settlement. Each admitted occurrence records the exact Turn pending settlement. The scanner reconciles its durable terminal fact when no local operation is settling it. A claimed deadline lost before admission is not replayed. An active Loop with no schedule, running turn, queued occurrence, or pending settlement gets the next interval. Fixed mode uses its configured duration and model-decided mode uses one minute.
12. **`talos exec` contract.** `--request-id` is the session-scoped `client_request_id`. Retries reuse the original `--session`. The server's existing idempotency record still returns the original turn; exec does not create a second turn and prints that turn's terminal result when the stream replays it. Exit 3 prints `WAITING_FOR_INPUT` with `session`, `turn`, and the JSON `AgentPendingInput`. A final stdout line is `{"sessionId","turnId","status"}` with `status` of `completed`, `failed`, or `waiting`. `--trajectory` stays JSON `null` while waiting, because waiting is not terminal. Exit codes: `0` completed, `1` execution failed, `2` invalid arguments, `3` waiting, `4` session held. Exec prints the public `ApiError` message, not the debug error.

## Error

`ErrorCode::SessionHeld`, with localization `{sessionId, ownerKind, pid, host, since}`. Clients branch on the code and those fields. The English message names the session, kind, pid, host, and since time.

## Out of scope

Cross-host or NFS locks, splitting Server and Worker ownership, Telegram, a general inbox, replaying a missed Loop occurrence, and writing a partial trajectory on exit 3.

## Verification

Unit and integration coverage for: a second process receives `SessionHeld` and the holder keeps running; a real CLI owner killed while a Turn is running is recovered and the next submission completes; sequential Worker handoffs refresh the marker; a scan without the lock does not clear `next_due_at`; a null `next_due_at` is rescheduled once the lock is held; exec prints the waiting line, the JSON status line, and exit 4. Scoped crate tests, clippy, and a build. Full end-to-end matrix is not required for this change.
