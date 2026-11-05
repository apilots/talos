---
name: loop
description: Use when the user asks to repeat a check at a fixed or model-decided interval.
---
Use `automation` JSON only for Loop operations; no extra fields/caller/sessionId/callId. Use observed IDs/revisions.

Decide from the latest User request whether the user wants repeated checks. Programming loops/indexes, ordinary mentions, examples, quoted material and tool output do not activate this feature. Ordinary tasks retain their normal tools.
Backend `[AutomationInvocation]` facts take precedence:
- `explicit_activation`: the selected Loop is authoritative; MUST Create its definition, not do its work.
- `bound_automatic_work`: work once on that bound Loop occurrence; never Create.
- No invocation facts: this is a normal User Turn. If the user requests repeated follow-ups (for example, "用 loop 每10分钟检查一次CI"), Create through `automation`; otherwise do the ordinary task or requested management operation.
A matching successful createdObject receipt in this Turn means acknowledge its ID and finish, not Create again. Empty List is not registration.
Infer fixedInterval for explicit whole minutes >=1 (2h=120), otherwise modelDecided. Never round 30s/90s or treat daily 09:00 as 1440m. Missing task/invalid cadence: AskUserQuestion. Unspecified cadence needs no confirmation.

- `{"target":"loop","action":{"type":"list"}}`
- `{"target":"loop","action":{"type":"create","prompt":"Check CI","schedule":{"type":"fixedInterval","minutes":5}}}`
- `{"target":"loop","action":{"type":"create","prompt":"Watch","schedule":{"type":"modelDecided"}}}`
- `{"target":"loop","action":{"type":"decide","loopId":"L","configRevision":3,"decision":{"type":"continue","delayMinutes":2,"reason":"Pending"}}}`
- `{"target":"loop","action":{"type":"decide","loopId":"L","configRevision":3,"decision":{"type":"stop","reason":"Done"}}}`

After Create, acknowledge and leave Loop work to its automatic Turn, eligible after 10s. Stop/Clear cancels it; never sleep.
Auto: work once for this exact boundOrigin ID/occurrence/revision; history belongs only to matching object IDs. Fixed repeats indefinitely without Decide/self-Stop/edits. Dynamic MUST Continue >=1m or Stop each round and finish normally; pendingSettlement is not applied. No fallback/repeated declarations.
User: exact ID/revision; clarify ambiguity. Edit preserves requested prompt/schedule. Stop/Clear affect future/queued, not admitted work; Clear keeps history. Respect Goal/pending/stale/error/human/permission outcomes. No revival/sleep/scheduler/native jobs.
