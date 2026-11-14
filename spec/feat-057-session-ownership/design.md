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

1. **The worker agent owns the lock.** `SessionOwner` lives in `talos-agent`, beside the session gate that admits runs (`orchestration/agent`). `TalosService` calls that object; it does not implement a second lock. TUI, CLI, Web, and Desktop only translate `SessionHeld`.
2. **One OS lock per session.** Non-blocking exclusive `talos_fs::try_file_lock` at `{data_dir}/worker/session-locks/{sha256(session_id)}.lock`, next to `worker/session-heads`. The kernel drops the lock when the process dies. No heartbeat and no TTL. Lock acquisition is the only source of truth.
3. **File contents are for the error text.** After a successful lock, write `{ownerKind, pid, host, since}`. `ownerKind` is `tui`, `exec`, `cli`, `server`, or `desktop`. A reader that cannot parse the file still returns `SessionHeld`.
4. **One in-process table.** The embedded runtime builds one `SessionOwner` and shares it with the worker RPC adapter and the service. A second `flock` in the same process is not a second owner: `holds` succeeds without opening the file again. Another process gets `SessionHeld` and does not take over or signal the holder.
5. **Hold scope is a runtime policy**, set when the process starts:

| Process | Policy | Maintenance scan |
| --- | --- | --- |
| TUI | `Attached`: one open session until switch or exit | on |
| `talos exec` | `Process`: hold until process exit | off |
| `goal start`, `loop start` | `Process` | on |
| Other short-lived CLI subcommands | `Turn` | off |
| `talos-server`, Desktop, SDK embed | `Turn`: admission through settlement | on |

`Turn` is the default for server, Desktop, and the SDK host so one process can serve many sessions. Web clients in that process do not each take a lock.

6. **Open versus read.** History reads do not take the lock. `recover_timeline` claims the open session for `Attached` and `Process`. A failed claim returns `SessionHeld`, loads no recovery, and does not settle. TUI keeps the previous owned session, shows the target history as read-only, prints who holds it, and does not retry the subscription. Web and Desktop stop the event reconnect loop on `SessionHeld`. HTTP maps the code to 409. `RunCleanupUnconfirmed` remains only for cleanup that this process still has in flight.
7. **Switching under a live turn.** `Attached` does not drop a session lock while this process still has a live turn there. The new session is not taken. After that turn settles, a later open can move the lock.
8. **Dead owner.** If this process acquires the lock and the canonical turn is not live here, the previous owner is dead. Interrupt treats cleanup as confirmed. A waiting continuation that already has a durable resume point still resumes. Every other nonterminal turn settles `Failed` with the public reason `owner process exited`. A live holder yields `SessionHeld` instead, and the turn keeps running.
9. **Start consults recovery before `ActiveTurnRunning`.** When the active turn is not live in this process, start recovers it. `SessionHeld` stops the start. A settled dead turn allows the new turn to continue. The same hold then lasts until that turn settles (`Turn`) or until process exit (`Process`).
10. **Loop scan.** Short-lived CLI, including exec, omits only the `session-loop-scan` job. Profile retention and dream maintenance still start. Where the scan runs, it asks `SessionOwner::try_for_scan` before the compare-and-set that clears `next_due_at`. The scan continues only when this process already holds the session, or when the policy is `Turn` and the lock is free. Otherwise it skips that session and writes nothing. A `Turn` hold taken only to scan is released if dispatch does not leave a live turn; a dispatched turn releases it at settlement.
11. **Missed Loop occurrence.** An occurrence already removed from `next_due_at` is not replayed. On a scan that holds the lock, an `active` Loop with `next_due_at = NULL`, no running turn, and no queued occurrence is rescheduled: fixed interval uses its minutes; model-decided uses the one-minute minimum. That is the next occurrence, not the lost one.
12. **`talos exec` contract.** `--request-id` is `client_request_id`. The server's existing idempotency record still returns the original turn; exec does not create a second turn and prints that turn's terminal result when the stream replays it. Exit 3 prints `WAITING_FOR_INPUT` with `session`, `turn`, and the JSON `AgentPendingInput`. A final stdout line is `{"sessionId","turnId","status"}` with `status` of `completed`, `failed`, or `waiting`. `--trajectory` stays JSON `null` while waiting, because waiting is not terminal. Exit codes: `0` completed, `1` execution failed, `2` invalid arguments, `3` waiting, `4` session held. Exec prints the public `ApiError` message, not the debug error.

## Error

`ErrorCode::SessionHeld`, with localization `{sessionId, ownerKind, pid, host, since}`. Clients branch on the code and those fields. The English message names the session, kind, pid, host, and since time.

## Out of scope

Cross-host or NFS locks, splitting Server and Worker ownership, Telegram, a general inbox, replaying a missed Loop occurrence, and writing a partial trajectory on exit 3.

## Verification

Unit and integration coverage for: a second process receives `SessionHeld` and the holder keeps running; dropping the owner's lock settles a non-resumable turn as `Failed` / `owner process exited`; a scan without the lock does not clear `next_due_at`; a null `next_due_at` is rescheduled once the lock is held; exec prints the waiting line, the JSON status line, and exit 4. Scoped crate tests, clippy, and a build. Full end-to-end matrix is not required for this change.
