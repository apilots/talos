# Profile Usage Statistics Design

Status: active
Owner: agent
Created: 2026-07-16
Updated: 2026-07-16
Related: `refactor-038-llm-model-selection`

## Background

The Profile screen currently derives counts, model usage, and token activity from
client-side Session and Project lists. That view is incomplete after historical
schema changes and cannot represent token usage for all logical turns. Provider
usage also has protocol-specific cache semantics that must not be silently
reported as zero.

## Goals

1. Return one user-scoped, durable Profile read model for HTTP, App Server,
   Rust SDK, TypeScript SDK, Web, Desktop, and future clients.
2. Persist one usage fact per logical `(session_id, turn_id)` and aggregate
   waiting/resume execution segments before making the fact visible.
3. Return lifetime usage, a 364-day daily series, top models, Session count,
   message count, and Project count.
4. Preserve provider usage semantics: OpenAI-style prompt counts already include
   cached tokens; Messages-style input counts must add cache read and cache
   creation tokens.
5. Surface unavailable provider usage explicitly instead of treating missing
   usage as a real zero.

## Non-goals

1. This feature does not introduce a custom transaction framework, outbox, or
   a new timeline broadcast order.
2. This feature does not add durable crash recovery for waiting/resume or a
   reusable retention scheduler. Those reliability concerns belong to the
   separate Profile reliability worktree.
3. This feature does not change the global `TokenUsage` integer width.
4. This feature does not repair historical usage data in local test databases.
   Existing users start accumulating Profile usage after the schema migration.
5. This feature does not expose Store records, SQL, or persistence details to
   Gateway, SDK, or UI callers.

## Architecture

`talos-store::profile` owns the SQLite record and the small `ProfileStore` API.
It accepts a typed `TurnUsageUpdate` and returns typed aggregate summaries.
`ProfileService` owns authenticated user scope, the timezone-specific calendar
window, and conversion into the public `ProfileReadResponse`. `TurnService`
only constructs a logical-turn update after an existing timeline event has been
persisted; it does not access SQL or Store records.

Each module is cohesive around one business capability and exposes the smallest
typed API needed for composition. The application layer wires modules together;
it does not expose storage state or add general-purpose dependency containers.

## Persistence and Migration

The Store migration adds `profile_turn_usage` with a unique
`(session_id, turn_id)` key. A row remains non-final while a logical turn waits
for user input. Subsequent execution segments are aggregated in that same row.
Only final rows are included in Profile reads. A cancelled or failed unfinished
turn removes its row.

The versioned SQL migration is owned by the Store migration runner. It changes
schema only; it never backfills, rewrites, or repairs historical Session data.
Historical local test database repairs remain a deliberate manual operation.

## Token Semantics and Availability

`TokenUsage.input_tokens` represents all billed input tokens. For Chat and
Responses protocols, provider prompt/input totals already include cached input;
cache fields are separate breakdowns and are never added again. For Messages
protocols, input, cache creation, and cache read are separate values, so all
three are added for input and total usage. Streaming Messages usage combines
the `message_start` input values with `message_delta` output values.

If the terminal turn has no provider usage values, Profile persists the row as
unavailable. Aggregates propagate that availability and clients render
`Unavailable` rather than a fabricated zero token total.
An intermediate waiting/resume segment with no model usage is neutral: it does
not erase usage or cache values already recorded for the logical turn.

## Public API and SDKs

The shared object model is defined once in `talos-types`:

```text
ProfileReadRequest {}
ProfileReadResponse
ProfileDailyUsage
ProfileModelUsage
```

HTTP exposes `GET /v1/profile`; App Server exposes `profile/read`. They call
the same Gateway method and return the same response object. Product clients do
not construct HTTP routes themselves:

- Rust SDK: `GatewayClient::read_profile(ProfileReadRequest)`.
- TypeScript App Server SDK: `AppServerClient.readProfile()`.
- TypeScript browser SDK: `TalosClient.profile.read()`.
- Web uses the browser SDK only; it has no `/v1/profile` string or HTTP mapping.

## Compatibility

The response is additive because Profile is a new API. Existing Session schemas
remain unchanged. Existing sessions remain visible in counts, but their past
usage is unavailable until newly completed turns produce Profile rows. No
fallback reconstructs token totals from Session state.

## Verification

Verification covers Store migration and aggregation behavior, timezone window
calculation, provider cache semantics, generated protocol artifacts, Rust SDK
and TypeScript SDK routing, and Web loading/error/empty rendering. The final
change also runs formatting, scoped tests, lint, and type checks.

## Follow-ups

The separate reliability worktree owns crash-safe usage recovery, generic stale
data retention and scheduling, transactional timeline/outbox behavior, and any
future global token-width migration. Those items must not be merged as implicit
Profile feature work.
