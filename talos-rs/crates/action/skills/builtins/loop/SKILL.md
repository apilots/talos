---
name: loop
description: Start or manage requested Loop follow-ups.
---
Use `automation` JSON only; no extra fields/caller/sessionId/callId. Use observed IDs/revisions.
Follow backend-confirmed intent (typed selection/explicit prompt); do not re-decide activation.
Read/List this Turn's boundOrigin. Route:
1. Matching successful createdObject receipt in this Turn: acknowledge ID/end.
2. boundOrigin: Auto, never Create.
3. New activation confirmed by backend: MUST Create the definition, not do the task. Null/empty Read/list is not registration/completion or success.
4. Otherwise explicit inspect/change-existing is management; never Create.
Infer fixedInterval for explicit whole minutes >=1 (2h=120), otherwise modelDecided. Never round 30s/90s or convert daily 09:00 to 1440m. Missing task/invalid cadence: AskUserQuestion in this logical Turn; later unselected Turns are unauthorized. Unspecified cadence needs no confirmation.

- `{"target":"loop","action":{"type":"list"}}`
- `{"target":"loop","action":{"type":"create","prompt":"Check CI","schedule":{"type":"fixedInterval","minutes":5}}}`
- `{"target":"loop","action":{"type":"create","prompt":"Watch","schedule":{"type":"modelDecided"}}}`
- `{"target":"loop","action":{"type":"decide","loopId":"L","configRevision":3,"decision":{"type":"continue","delayMinutes":2,"reason":"Pending"}}}`
- `{"target":"loop","action":{"type":"decide","loopId":"L","configRevision":3,"decision":{"type":"stop","reason":"Done"}}}`

After Create, no work this registration Turn. Service starts first work after 10s; Stop/Clear cancels it. Never sleep.
Auto: never Create; work once for this Turn's exact boundOrigin ID/occurrence/revision. Attribute history by matching Tool object IDs; other IDs and prompt examples (LOOP_1/LOOP_2) are not this Loop's progress. A new ID starts its own first work.
Fixed: no Decide/self-Stop/edits. Dynamic MUST Continue >=1m or Stop each round, then finish normally; pendingSettlement is not applied. No repeated declarations/fallback.
User: exact ID/revision; clarify ambiguity; preserve prompt/schedule. Stop/Clear affect future/queued, not admitted work; Clear keeps history. Auto cannot edit/clear/management-Stop; Steer bound-only. Respect Goal/pending/stale/error/human/permission. No revival/sleep/scheduler/native jobs.
