# Talos application workspace

[Chinese](README_zh.md)

`talos-app` contains the React Web UI, Electron Desktop shell, and TypeScript
SDK packages. Product features consume shared SDK domain objects and backend
semantics instead of defining client-specific business models.

## Layout

```text
talos-app/
|-- web/                  React 19 and Vite Web UI
|-- desktop/              Electron shell using the shared Web UI
|-- ts-sdk/packages/
|   |-- core/             Generated protocol types and client primitives
|   |-- browser/          HTTP and SSE adapter
|   `-- app-server/       JSON-RPC stdio, Electron, and IPC adapters
|-- scripts/              Build and verification helpers
`-- pnpm-workspace.yaml   Workspace definition
```

## Requirements

- Node.js 24 recommended; 20.19.0 is the minimum supported 20.x release
- pnpm 9.14.4 or a compatible repository version

## Run the Web UI

From the repository root, start the server and Web UI in separate terminals:

```bash
just dev-server
just dev-web
```

The development UI is available at `http://127.0.0.1:5173` and connects to the
Talos server at `http://127.0.0.1:8080` by default.

## Run Desktop

```bash
pnpm --dir talos-app desktop:dev
pnpm --dir talos-app desktop:check
```

Desktop starts `app-server` in the Electron main process and communicates over
JSON-RPC stdio. Renderer code uses the typed preload bridge and cannot access
Node.js or the filesystem directly.

## Validate

```bash
pnpm --dir talos-app lint
pnpm --dir talos-app test
pnpm --dir talos-app build
```

Use package filters for local iteration. Generated protocol files are refreshed
from Rust with `just gen-app-protocol`; do not edit them manually.

## Boundary rules

- UI features consume `@talos/sdk-browser` or `@talos/sdk-app-server`.
- Shared protocol and domain types come from `@talos/sdk-core`.
- Browser and App Server facades must expose aligned TypeScript SDK capability families.
- TypeScript, Rust, and embedded Rust SDK capabilities are reviewed and completed together.
- Backend search, matching, lifecycle, and permission rules are not duplicated in UI code.
- User-visible timeline changes require rendering and interaction coverage.

See [Client architecture](../docs/architecture/CLIENT_ARCHITECTURE.md) for the
cross-client transport and ownership model.
