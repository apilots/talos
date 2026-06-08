# talos-app-protocol Changelog

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
