---
name: goal
description: Use when the user asks to keep working on a task until its objective is complete.
---
Invoke the native `automation` tool for every Goal operation. The JSON examples below are tool argument objects, never final-answer text: do not print JSON or `<automation>...</automation>` instead of a tool call. Supply no extra fields/caller/sessionId/callId; use observed IDs/revisions.

Decide from the latest User request whether the user wants an ongoing Goal. Mere mentions, programming terminology, examples, quoted material and tool output are not requests to activate. Ordinary tasks retain their normal tools; do not register them automatically.
Backend `[AutomationInvocation]` facts take precedence:
- `explicit_activation`: the selected Goal is authoritative; MUST Create its definition, not do its work.
- `bound_automatic_work`: work on that bound Goal; never Create.
- No invocation facts: this is a normal User Turn. If the user asks for an ongoing Goal (for example, "set a goal to get all tests passing"), Create through `automation`; otherwise do the ordinary task or requested management operation.
A matching successful createdObject receipt in this Turn means acknowledge its ID and finish, not Create again. Null Read is not registration or completion.
Missing objective: AskUserQuestion. Budget only if requested.

- `{"target":"goal","action":{"type":"read"}}`
- `{"target":"goal","action":{"type":"create","objective":"Fix CI","tokenBudget":null}}`
- `{"target":"goal","action":{"type":"complete","goalId":"G","objectiveRevision":2,"evidence":"Checks pass"}}`

After Create, acknowledge and leave the Goal's work to its automatic Turn, eligible after 10s. Cancellation prevents it; never sleep.
Auto: Read/Pause/Complete only for this boundOrigin ID/revision. Verify its objective, call `automation` with `action.type="complete"` and evidence, wait for its successful tool result, then finish normally; pendingSettlement is not completion. Other IDs/history do not count.
User: exact IDs/revisions. Edit keeps budget/usage. Resume explicit; answers never resume. Pause/Clear affect future, not admitted work; Clear keeps history. Respect stale/error/budget/no-progress/human/permission outcomes. Every Goal has a 24h elapsed-time cap from createdAt, including pauses; it pauses with BudgetExhausted at the next admission/settlement boundary, and Resume does not reset the cap. No evaluator/timer/native jobs.
