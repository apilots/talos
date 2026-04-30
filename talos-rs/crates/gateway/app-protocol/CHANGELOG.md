# talos-app-protocol Changelog

## 0.2

- Add frozen app-server methods for `session.turns` and `project.create/list/read/patch/delete`.
- Keep `tool.dynamic_result.submit`, `admin.session.list`, and `admin.project.list` in `DEFERRED_CLIENT_METHODS` until server-request and local actor credential boundaries are fully stabilized.
- Refresh JSON Schema and generated TypeScript fixtures from Rust protocol types.
- Cover project, memory file, workspace docs, replay, multi-subscriber, and error mapping flows in app-server protocol e2e.

## 0.1

- Establish JSON-RPC 2.0 base message types.
- Add initial client request params for initialize, session, turn, event, tool approval, model, config, memory, workspace docs, skills, apps, tools, account, and memory search methods.
- Add canonical server notification and server request method types.
- Add strong JSON-RPC business error data.
- Add JSON Schema and TypeScript generated fixtures.

Version rules:

- Breaking field or behavior changes bump `ProtocolVersion.major`.
- Additive optional fields, methods, or notifications bump `ProtocolVersion.minor`.
- Generated schema and TypeScript fixtures must be updated in the same change as Rust protocol types.
