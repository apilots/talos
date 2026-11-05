---
name: goal
description: Start or manage a requested Goal.
---
Use `automation` JSON only; no extra fields/caller/sessionId/callId. Use observed IDs/revisions.
Follow backend-confirmed intent (typed selection/explicit prompt); do not re-decide activation.
Read/List this Turn's boundOrigin. Route:
1. Matching successful createdObject receipt in this Turn: acknowledge ID/end.
2. boundOrigin: Auto, never Create.
3. New activation confirmed by backend: MUST Create the definition, not do the task. Null/empty Read/list is not registration/completion or success.
4. Otherwise explicit inspect/change-existing is management; never Create.
Missing objective: AskUserQuestion in this logical Turn; a later unselected Turn is unauthorized. Budget only if requested.

- `{"target":"goal","action":{"type":"read"}}`
- `{"target":"goal","action":{"type":"create","objective":"Fix CI","tokenBudget":null}}`
- `{"target":"goal","action":{"type":"complete","goalId":"G","objectiveRevision":2,"evidence":"Checks pass"}}`

After Create, no work/Complete this registration Turn even if boundOrigin set. Service starts first work after 10s; cancellation prevents it. Never sleep.
Auto: never Create; Read/Pause/Complete only for current boundOrigin ID/revision. Progress/evidence from other IDs or example outputs does not count. Verify this objective, Complete, finish normally; pendingSettlement is not completion.
User: exact IDs/revisions. Edit keeps budget/usage; Resume explicit, answers never resume. Pause/Clear affect future, not admitted work; Clear keeps history. Bound-only Steer. Respect stale/error/budget/no-progress/human/permission. No evaluator/timer/native jobs.
