# Web UI Gap Register

## Scope

This file records backend and platform gaps discovered while implementing the Web UI on top of the existing shared Talos APIs.
No items below were implemented in this round.

## Gaps

### 1. Project CRUD APIs

- Surface: Chat welcome page, project switcher, project bar.
- Current limitation: the server does not expose project list/create/update/delete APIs.
- Current UI handling: project registry is persisted locally in the browser store, with one normalized entry for the current workspace path and one entry for the server default workspace.

### 2. Memory CRUD APIs

- Surface: `/memory`.
- Current limitation: there is no shared HTTP API for listing, editing, or deleting memory entries in the current Web MVP scope.
- Current UI handling: the page renders locally persisted placeholder records so layout and interaction can be debugged end-to-end.

### 3. Files CRUD APIs

- Surface: `/files`, composer `@` affordance, upload entry.
- Current limitation: there is no shared HTTP API for file list/upload/delete in the current Web MVP scope.
- Current UI handling: the page renders locally persisted placeholder records and exposes UI-only upload affordances.

### 4. Admin APIs

- Surface: `/admin`.
- Current limitation: there are no shared admin endpoints for dashboard, users, sessions, audit log, models, or system status in this round.
- Current UI handling: the page derives a small subset of metrics from existing `/v1/models` and `/v1/sessions` queries, and keeps the rest as static placeholders.

### 5. Session Rename API

- Surface: session title actions.
- Current limitation: there is no shared endpoint for renaming a session title from the Web UI.
- Current UI handling: rename remains a local alias stored in the browser only.

### 6. Replayable Session Stream API

- Surface: durable session replay, reconnect semantics, status recovery after refresh.
- Current limitation: the current shared API only provides request-bound message streaming and canonical `/turns` history, not a replayable per-session transcript event stream.
- Current UI handling: live blocks are rendered from the current request stream only; after refresh the UI falls back to `/turns`.

### 7. Global Event Stream

- Surface: global notifications, background task feed, cross-session badges.
- Current limitation: there is no shared global events endpoint in the current MVP scope.
- Current UI handling: sidebar status and connection hints are derived locally from active page activity.

### 8. Webapp-Testing Python Runtime

- Surface: local browser automation workflow.
- Current limitation: the `webapp-testing` helper script could not be used end-to-end because the local Python environment does not have the `playwright` module installed.
- Current UI handling: real browser validation was completed with the Playwright MCP tooling instead.

## Phase 2 Priorities

### P0. Project APIs

- Scope: project list, create, update, delete, and session binding.
- Why first: chat home, project switcher, and settings already expose project concepts, but they are still browser-local state.

### P0. Memory APIs

- Scope: list, create, update, delete durable memories.
- Why first: `/memory` already has a complete UI surface and should be converted from local placeholder state to real data with minimal UI churn.

### P1. Files APIs

- Scope: file list, upload, delete, and lightweight metadata for the files page and composer mentions.
- Why next: `/files` and the composer affordances are visible product surfaces, but can tolerate one phase of placeholder behavior longer than memory.

### P1. Session Metadata APIs

- Scope: rename session title, richer status recovery, and server-truth session labels.
- Why next: this closes the remaining browser-local aliases and improves parity with long-running session workflows.

### P2. Replayable Session Stream

- Scope: reconnectable per-session transcript stream and durable progress recovery after refresh.
- Why later: the current Web UI now has acceptable detached-state handling on top of `/turns`, but true resume semantics still require a dedicated backend design.

### P2. Admin APIs

- Scope: dashboard, audit, models, users, and system data for `/admin`.
- Why later: admin is useful, but it is less critical than project, memory, files, and session truth for the current Web console.
