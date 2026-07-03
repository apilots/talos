# Session Metadata and Model Selection Plan

## 阶段 1: TUI Turn Metadata Parity
**目标**: Make TUI send the same required turn metadata on every turn that CLI already sends through the normal request path.

**成功标准**:
- Every TUI turn includes session context, `current_directory`, and structured `model_ref` when a current model is selected.
- Non-first TUI turns can still trigger tools in the same session.
- TUI current directory remains the session-bound directory for the terminal workspace.

**测试**:
- `test_submit_input_sends_turn_metadata_after_first_turn`
- `08_tui/embedded_write_html.sh`

**状态**: 已完成

## 阶段 2: Structured Model Selection
**目标**: Replace request-level string model selection with `model_ref { provider, id }` across API, SDK, CLI, TUI, Web, Desktop, and E2E clients.

**成功标准**:
- Turn requests carry `model_ref` instead of legacy `model`.
- Missing `model_ref` uses the current configured default provider and model.
- Explicit `model_ref` persists through session configuration and is applied before dispatch.
- `TalosContext.provider` is not used for LLM provider selection.

**测试**:
- Service tests for default model tracking and explicit model selection.
- TUI command/input tests for structured model selection.
- Web/desktop typecheck and scoped web tests.
- Updated E2E payload helper and affected scenarios.

**状态**: 已完成

## 阶段 3: Session Service Boundary Cleanup
**目标**: Keep DB records and store objects inside the store/service boundary, and avoid exposing persistence-specific names through service APIs.

**成功标准**:
- `SessionRecord` remains a store/repository object.
- `SessionService` exposes business operations and maps records internally.
- Turn progress recording uses a narrow service-facing interface, not a store record service.
- Removed temporary `record_service`, `model_projection`, and persistence-style abstractions.

**测试**:
- `just check -p talos-service ...`
- `just test -p talos-service`

**状态**: 已完成

## 阶段 4: Model Configuration Cleanup
**目标**: Remove redundant LLM provider-level `enabled` configuration. Default provider and model definitions express active model selection.

**成功标准**:
- YAML config files no longer contain provider-level LLM `enabled`.
- Backend config types no longer include provider-level LLM `enabled`.
- Frontend settings no longer renders or writes provider-level LLM `enabled`.
- Generated app protocol/schema and SDK types are updated.

**测试**:
- Config tests.
- Web settings editor test.
- TypeScript typecheck.

**状态**: 已完成

## 阶段 5: Per-run LLM Provider Routing
**目标**: Introduce `LlmProviderRouter` as the runtime LLM provider selection boundary, so every Agent run uses the effective `ModelRef { provider, id }` selected by config, Agent default, session state, or turn request.

**成功标准**:
- Downstream Agent/Harness/Kernel code holds `LlmProviderRouter` instead of a single global `LlmProvider`.
- Each run resolves the concrete provider from `AgentRunConfig.llm.provider` before invoking the LLM.
- Same-provider model switching and cross-provider model switching both call the expected provider instance.
- No non-streaming fallback or provider guessing is introduced; unsupported provider/model combinations fail explicitly.
- Session model selection is represented as `SessionConf.model_ref`, not `SessionConf.agent.provider/model`.
- Agent default model selection uses explicit `agent.model_ref` when configured, otherwise derives from global `llm.provider/model`.
- Removed `agent.model`, `SessionConf.agent`, `conf.agent.model`, and related tests/fixtures instead of keeping compatibility paths.

**测试**:
- Service/Agent integration test with two mock providers proving cross-provider `model_ref` dispatch reaches the selected provider.
- Session tests for priority order: turn request `model_ref` > persisted session `model_ref` > agent `model_ref` > global `llm.provider/model`.
- Config tests for `agent.model_ref` derivation and removal of legacy `agent.model`.
- Existing TUI/CLI/Web/Desktop model-switch tests updated to assert full `ModelRef` payloads.
- Existing E2E session lifecycle scenario updated to patch `conf.model_ref`.

**状态**: 已完成

## 阶段 6: Verification
**目标**: Verify the original regression and the touched integration surfaces without adding broad low-signal E2E coverage.

**成功标准**:
- Rust scoped check and tests pass.
- Frontend/desktop typecheck passes.
- Scoped web tests pass.
- Existing TUI E2E case catches the second-turn tool-call behavior.
- Cross-provider session model selection proves the selected provider instance is used.
- Legacy `agent.model` and `SessionConf.agent` references are removed from schemas, SDKs, frontends, E2E payloads, and Rust tests.

**测试**:
- `just check -p talos-service -p talos-agent -p talos-tui -p talos-types -p talos-config -p talos-base -p talos-app-server-test-client -p talos-store -p talos-gateway-http`
- `just test -p talos-types -p talos-service -p talos-agent -p talos-config -p talos-store -p talos-tui`
- `pnpm --dir talos-app/web typecheck`
- `pnpm --dir talos-app/desktop typecheck`
- `pnpm --dir talos-app/web test -- --run src/features/chat/api.test.ts src/pages/chat/ChatSessionPage.test.tsx src/features/settings/components/models/SettingsModelsEditor.test.tsx src/shared/api/desktop-client.test.ts`
- `find talos-e2e/scenarios talos-e2e/scripts -name '*.sh' -print0 | xargs -0 -n1 bash -n`
- `just e2e-case 08_tui/embedded_write_html`

**状态**: 已完成
