# MCP exit cleanup on Darwin

Status: implemented

## Problem and root cause

During normal stdio/transport shutdown a native MCP process can be in Darwin's
proc_exit transition. killpg(SIGKILL) then returns EPERM because no group member
can accept the signal, while waitpid(WNOHANG) still returns zero: root termination
is not yet waitable. A fully waitable zombie is a later state, and an already
reaped group normally returns ESRCH.

The existing cleanup only retries a failed group signal when a single try_wait
already returns Some(status). It treats the earlier exit transition as a terminal
permission failure, returning before the process wrapper can finish reaping.
Repeated successful Desktop shutdowns cannot rule out this race.

## Ownership and fix

Keep process-wrap's ProcessGroup/JobObject, Tokio Child and the existing MCP
Cleanup TaskTracker as the only owners. On Darwin EPERM, await the existing
wrapper's wait within the cleanup deadline, then retry the group signal before
accepting cleanup. A still-inaccessible group must retain its original typed
permission error. Root waiting and final group waiting share one bounded cleanup
operation; do not create a retry manager, periodic probe, detached reaper or
message-based exception.

Explicit shutdown borrows the child from its existing McpProcess Drop guard until
cleanup completes, so cancelling the wait still transfers ownership into the
original tracked Drop path. Taking the child before awaiting would bypass that
recovery and could leave the stderr drain task alive.

Explicit shutdown owns process cleanup. Cancellation before publication uses the
existing tracked Drop cleanup. Stderr drains are cancelled through their existing
abort handle, and the enclosing TaskTracker drain is the completion barrier.
No root PID, group or task may be treated as cleaned solely because an error was
logged or a later repeated scenario passed.

## Verification

A native syscall probe distinguishes killpg EPERM with a not-yet-waitable root
from a stable zombie. A controlled ChildWrapper regression holds the real root
until wait is awaited, reproducing the observed API ordering deterministically.
It must fail against the one-shot try_wait implementation. Preserve a separate
case where permission denial survives root reaping.

Use real child groups for explicit shutdown and cancelled-startup Drop cleanup;
verify root and descendant disappearance, TaskTracker emptiness and owner release.
The existing Linux Rust boundary job runs the owning crate, and a native macOS
Rust job exercises the Darwin-only regression. Run the complete MCP lifecycle suite and actual Desktop stdio recovery/shutdown on
immutable binaries built in a task-specific target. Retain actual results in
ignored output and the PR. Logging rotation changes belong only to PR55.

## Constraints

No change to MCP tool admission, authentication, reconnect or any fixed five-second
registration retry. No privileged process is killed to simulate permission denial;
keep that boundary typed and controlled in the regression. Processes escaping the
owned OS process group/job are outside the existing process-wrap guarantee.
