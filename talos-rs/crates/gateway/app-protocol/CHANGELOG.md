# talos-app-protocol Changelog

## 0.5

- Add owner-only `computer/delete` with HTTP and SDK parity. Deletion revokes remote
  registration while retaining local files and historical resource bindings.
- Add typed localization for active-Turn deletion refusal, removed registration,
  and unverified admission. Update all protocol consumers together.

## 0.4

- ApiError now carries specific English text and required nullable localization.
- AgentTurnFailed reuses ApiError; retryable is a sibling of error.
- HTTP no longer recodes missing resources, retry validation, or project deletion
  failures into route-specific codes. It preserves the same service code and
  explanation as RPC. Timeline cursor owners emit InvalidCursor/EventCursorExpired
  directly for all transports; generic request failures remain InvalidRequest.
- Initialization rejects incompatible protocol versions. Deploy all consumers
  together and run the SQLx failure-event migration; see ERROR_MODEL.md for rollback.

## 0.3

- Add app-server parity methods for `session.compact`, `session.clear`, `session.active_turn`, `session.history`, and `turn.retry`.
- Split stable session history snapshots from active-turn live recovery snapshots.
- Freeze `tool.dynamic_result.submit`; `DEFERRED_CLIENT_METHODS` is empty after P2 protocol completion.
- Refresh JSON Schema and generated TypeScript fixtures from Rust protocol types.
- Extend app-server protocol resource matrix coverage for admin list and dynamic tool result error mapping.

## 0.2

- Add frozen app-server methods for session and project CRUD.
- Refresh JSON Schema and generated TypeScript fixtures from Rust protocol types.
- Cover project, memory file, workspace docs, replay, multi-subscriber, and error mapping flows in app-server protocol e2e.

## 0.1

- Establish JSON-RPC 2.0 base message types.
- Add initial client request params for initialize, session, turn, event, dynamic tool results, model, config, memory, workspace docs, skills, apps, tools, account, and memory search methods.
- Add canonical server notification and server request method types.
- Add strong JSON-RPC business error data.
- Add JSON Schema and TypeScript generated fixtures.

Version rules:

- Breaking field or behavior changes bump `ProtocolVersion.major`.
- Additive optional fields, methods, or notifications bump `ProtocolVersion.minor`.
- Generated schema and TypeScript fixtures must be updated in the same change as Rust protocol types.
