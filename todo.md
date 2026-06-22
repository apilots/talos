# Session / Model / Config API Object TODO

Status: completed
Parent plan: `IMPLEMENTATION_PLAN.md`
Related spec: `spec/refactor-027-core-api-object-boundary/design.md`
Created: 2026-06-21

## Goal

Finish the remaining public object cleanup for session, model, and config APIs.

The target is to remove TS SDK local business DTOs for these areas and make HTTP,
JSON-RPC, Rust SDK, TS SDK, Web, and Desktop share the same `talos_types` objects.

## Design Decisions

- `Session` is the internal server execution object. It replaces the current internal `TalosSession` concept and must not be exported to SDK protocol types.
- `SessionInfo` is the public session DTO returned by Gateway / HTTP / JSON-RPC / SDK APIs.
- `ModelInfo` is a public DTO for `model.list`; it is not the full model config structure.
- `LlmConfig` is the public LLM config object used by config APIs and UI settings.
- Public API method names should use `get`, `update`, and `delete`; avoid adding new `read` names.
- Public request / response objects live in `talos_types`.
- `talos_types` must not depend on `talos_config`.
- Application Service owns conversion between internal objects and public DTOs.

## Object Boundary

```text
SessionRecord
  -> Session
  -> SessionInfo
  -> Gateway / HTTP / JSON-RPC / SDK
```

```text
talos_config internal config
  -> LlmConfig / ModelInfo
  -> Gateway / HTTP / JSON-RPC / SDK
```

## Phase 1: Public Type Baseline

**Goal**: Define the public object set in `talos_types`.

**Deliverables**:
- `SessionInfo`
- `SessionListRequest` / `SessionListResponse`
- `SessionGetRequest` / `SessionGetResponse`
- `SessionUpdateRequest` / `SessionUpdateResponse`
- `SessionDeleteRequest` / `SessionDeleteResponse`
- `SessionClearRequest` / `SessionClearResponse`
- `SessionCompactRequest` / `SessionCompactResponse`
- `ModelListRequest` / `ModelListResponse`
- `ModelInfo`
- `LlmConfig`
- `LlmProviderConfig`
- `LlmModelConfig`
- `LlmModelCost`
- `LlmModelCompat`
- `LlmModelInputKind`
- `LlmModelSpeed`
- `LlmReasoningEffort`
- `ConfigGetRequest` / `ConfigGetResponse`
- `ConfigUpdateRequest` / `ConfigUpdateResponse`
- `ConfigDeleteRequest` / `ConfigDeleteResponse`
- `ConfigReloadRequest` / `ConfigReloadResponse`

**Success Criteria**:
- `talos_types` exports all public objects needed by app-protocol.
- No public DTO in `talos_types` imports `talos_config`.
- TS protocol generation includes the new public objects.

**Tests**:
- `cargo check -p talos-types -p talos-app-protocol`
- `cargo test -p talos-app-protocol`

**Status**: completed

**Completed**:
- Added shared `SessionInfo`, session response wrappers, `ModelListResponse`, public `Llm*` DTOs, and `ConfigGet/Update/Delete/Reload` objects.
- Regenerated app-protocol schema and TS generated types.

## Phase 2: Session API Migration

**Goal**: Stop exposing internal session objects through Gateway and SDK APIs.

**Deliverables**:
- Rename internal `TalosSession` concept to `Session` if the codebase scope is manageable.
- Add Application Service mapping from `SessionRecord` / internal `Session` to `SessionInfo`.
- Change Gateway session methods to return `SessionInfo` response objects.
- Change HTTP and JSON-RPC session APIs from read-style names to get-style names where protocol compatibility allows.
- Keep history and active-turn APIs on their existing dedicated public objects.

**Success Criteria**:
- Public session APIs do not return internal `Session`.
- TS SDK no longer defines local `TalosSession`, `SessionRecord`, `SessionQuery`, or `SessionUpdate` business DTOs.
- Session list/get/update/delete/clear/compact responses use shared generated types.

**Tests**:
- `cargo test -p talos-types -p talos-application-service -p talos-gateway-http -p talos-app-server -p talos-sdk`
- `pnpm --filter @talos/sdk-core test`
- `pnpm --filter @talos/sdk-core build`
- `pnpm --filter talos-web typecheck`
- `pnpm --filter talos-desktop typecheck`

**Status**: completed

**Completed**:
- Gateway / HTTP / JSON-RPC / Rust SDK / TS SDK no longer expose internal session objects for list/get/update/compact/clear.
- Public session API uses `session.get` and shared response objects.

## Phase 3: Model API Migration

**Goal**: Keep `ModelInfo` as a public DTO while removing its dependency on `talos_config` types.

**Deliverables**:
- Define model DTO subtypes in `talos_types`.
- Map internal config model entries to public `ModelInfo` in Application Service or shared model helper code.
- Return `ModelListResponse` from Gateway / HTTP / JSON-RPC / SDK.
- Remove TS SDK local `ModelInfo` and related model subtype definitions.

**Success Criteria**:
- `ModelInfo` is generated from `talos_types`.
- `ModelInfo` does not expose the full model config structure.
- `talos_types` does not re-export `talos_config` model config types.

**Tests**:
- `cargo check -p talos-types -p talos-application-service -p talos-gateway-http -p talos-app-server`
- `pnpm --filter @talos/sdk-core build`
- `pnpm --filter talos-web typecheck`

**Status**: completed

**Completed**:
- `model.list` returns shared `ModelListResponse`.
- TS SDK removed local `ModelInfo` and uses generated `talos_types` output.

## Phase 4: Config API Migration

**Goal**: Provide generic config APIs while keeping strongly typed public config objects.

**Deliverables**:
- Add `config.get`, `config.update`, `config.delete`, and `config.reload` public request / response objects.
- Define `LlmConfig` public objects in `talos_types`.
- Map between `talos_config` internal config and public `LlmConfig`.
- Keep UI usage focused on LLM provider config without exposing the full internal config schema.
- Replace old `ConfigRead*` names with `ConfigGet*` names.

**Success Criteria**:
- Config APIs use get/update/delete naming.
- Public config objects are generated from `talos_types`.
- TS SDK no longer defines local `LlmConfig`, provider config, model config, readiness, or key status DTOs.

**Tests**:
- `cargo test -p talos-types -p talos-application-service -p talos-gateway-http -p talos-app-server`
- `pnpm --filter @talos/sdk-core test`
- `pnpm --filter @talos/sdk-core build`
- `pnpm --filter talos-web typecheck`

**Status**: completed

**Completed**:
- Public config API uses `config.get`, `config.update`, `config.delete`, and `config.reload`.
- Removed old `config.read`, `config.value.write`, and `config.batch.write` public method names and compatibility aliases.

## Phase 5: SDK / UI Cleanup And Regression

**Goal**: Remove duplicate client-side DTOs and verify all protocols use shared types.

**Deliverables**:
- Delete obsolete TS SDK local DTOs from `types.ts`.
- Remove temporary adapters and normalizers that only existed for local DTO compatibility.
- Update Web / Desktop imports to shared generated SDK types.
- Update docs and tests to use `get` naming.

**Success Criteria**:
- `rg` finds no TS SDK local business DTOs for session, model, or config.
- HTTP and JSON-RPC expose equivalent request / response objects.
- Rust SDK and TS SDK expose the same business object names.
- No compatibility branch remains for old read/config/session DTO names unless explicitly documented as a protocol alias.

**Tests**:
- `cargo test -p talos-types -p talos-application-service -p talos-gateway-http -p talos-app-server -p talos-sdk`
- `cargo check -p talos-cli -p talos-ui-tui -p talos-base`
- `pnpm --filter @talos/sdk-core test`
- `pnpm --filter @talos/sdk-core build`
- `pnpm --filter @talos/sdk-browser build`
- `pnpm --filter talos-web typecheck`
- `pnpm --filter talos-desktop typecheck`
- App-server e2e when the environment is available.

**Status**: completed

**Completed**:
- Rust SDK, TS SDK, Web, Desktop, CLI, TUI, and app-server test client are wired to shared response objects.
- Removed TS SDK local session/model/config business DTO definitions.
- Desktop IPC guard allows only the new config/session method names.

**Verified**:
- `cargo check -p talos-app-protocol -p talos-gateway -p talos-gateway-http -p talos-app-server -p talos-sdk -p talos-cli -p talos-ui-tui -p talos-app-server-test-client`
- `cargo check -p talos-channel`
- `cargo test -p talos-gateway-http`
- `cargo test -p talos-app-protocol -p talos-gateway -p talos-app-server -p talos-sdk`
- `pnpm --filter @talos/sdk-core test`
- `pnpm --filter @talos/sdk-core build`
- `pnpm --filter @talos/sdk-browser build`
- `pnpm --filter @talos/sdk-app-server build`
- `pnpm --filter talos-web typecheck`
- `pnpm --filter talos-desktop typecheck`
- `pnpm --filter talos-desktop test`

## Closed Questions

- Old JSON-RPC method aliases were removed for this refactor target state.
- `ConfigDeleteRequest` is exposed as the generic public API shape; current `llm` delete fails explicitly because the required LLM config cannot be deleted.
- Public LLM config DTOs live in `talos_types` with model DTOs because `ModelInfo` and `LlmConfig` share the same public provider/model schema boundary.
