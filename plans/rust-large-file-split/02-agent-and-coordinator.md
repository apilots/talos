## 阶段 1: Baseline And Boundary Freeze
**目标**: Confirm the stable runtime, turn, event bridge, approval, flush, request-handler, session-service, and event-stream boundaries before moving orchestration code.
**成功标准**: `agent.rs` and `coordinator.rs` responsibilities are mapped to target modules, `MockAgent` is identified as test-only infrastructure, and the target directory layout is fixed for both crates.
**测试**: `cargo test -p talos-agent` and `cargo test -p talos-coordinator`
**状态**: 已完成

### Baseline Line Counts

Current oversized file sizes before any split work:

- `agent/src/agent.rs`: `1832`
- `coordinator/src/coordinator.rs`: `1739`

Files already at acceptable size (no split needed):

- `agent/src/flush_manager.rs`: `262`
- `agent/src/service.rs`: `174`
- `agent/src/tests.rs`: `144`
- `agent/src/trait.rs`: `14`
- `agent/src/types.rs`: `21`
- `coordinator/src/trait.rs`: `14`
- `coordinator/src/types.rs`: `34`

### Frozen Export Surface

`agent/src/lib.rs` current public surface:

```
pub use agent::AgentImpl as LLMAgent;
pub use agent::{AgentImpl, MockAgent};
pub use r#trait::Agent;
pub use service::{
    AgentOrchestrator, AgentRequest, OrchestratorEvent, OrchestratorExecution, OrchestratorRequest,
};
pub use talos_agent_kernel::TurnPhase;
pub use types::{AgentResponse, AgentStream, ToolCall};
```

`coordinator/src/lib.rs` current public surface:

```
pub use coordinator::CoordinatorImpl;
pub use r#trait::Coordinator;
pub use types::{CoordinatorResponse, CoordinatorStatus};
pub mod prelude { ... }
```

The implementation requirement is to preserve this crate-level re-export surface unchanged unless a dedicated cleanup phase explicitly removes or renames an item.

### Boundary Freeze Notes — `agent.rs`

Logical sections identified inside the 1832-line file:

| Lines | Content |
| --- | --- |
| 1–75 | Imports, `KernelExecutionMode` enum, `ActiveTurnHandle` struct, `AgentImpl` struct field definition |
| 76–351 | `impl AgentImpl` construction block: `new`, `normalized_kernel_config`, `build_memory_store_from_kernel_config`, `rebuild_memory_store`, builder setters (`add_tool`, `set_tool_registry`, `set_default_planner_mode`, `set_prompt_budget`, `update_kernel_config`) |
| 354–460 | `impl AgentImpl` dead-code block: `process_tool_calls`, `derive_exec_command`, `generate_system_prompt` |
| 461–558 | `impl AgentImpl` kernel agent factory: `build_llm_kernel_agent`, `kernel_config_snapshot`, `resolve_kernel_mode`, `build_kernel_agent_for_request` |
| 560–763 | `impl AgentImpl` event adaptation: `map_kernel_stream_event`, `submit_tool_approval`, `submit_dynamic_tool_result`; standalone: `map_thinking_source`, `map_thinking_stage` |
| 765–832 | `impl Agent for AgentImpl` (`execute_tool`) |
| 834–1146 | `impl AgentOrchestrator for AgentImpl`: `run`, `run_stream`, `close_session`, `drain_memory_flushes`, `pending_memory_flushes`, `cancel_active_turn`, `submit_tool_approval`, `submit_dynamic_tool_result` delegations |
| 1148–1326 | `MockAgent` struct + `impl Agent`, `impl KernelAgent`, `impl AgentOrchestrator` for `MockAgent` |
| 1328–1832 | `#[cfg(test)] mod tests` |

Target split for `agent/`:

```
mod.rs            ← thin facade: mod declarations + pub use
types.rs          ← KernelExecutionMode, ActiveTurnHandle
builder.rs        ← AgentImpl struct + new() + config/tool builder methods
kernel_bridge.rs  ← kernel agent factory + stream event mapping + thinking mappers
orchestrator.rs   ← impl Agent + impl AgentOrchestrator (run, run_stream, helpers)
tool_submission.rs← validate_dynamic_tool_submission + submit_tool_approval + submit_dynamic_tool_result
mock.rs           ← MockAgent + all its trait impls
tests.rs          ← #[cfg(test)] mod tests
```

Key helper methods and their target file:

- `resolve_selected_model_info`, `map_stop_reason`, `extract_text_input`, `collect_reasoning`, `to_agent_request` → `orchestrator.rs`
- `register_active_turn`, `clear_active_turn` → `orchestrator.rs` (called from run + run_stream)
- `cancel_active_turn`, `drain_pending_memory_flushes`, `pending_memory_flush_count` → `orchestrator.rs` (delegated by trait impls)
- `validate_dynamic_tool_submission` → `tool_submission.rs`
- `process_tool_calls`, `derive_exec_command`, `generate_system_prompt` → `builder.rs` (dead code, keep together with construction helpers)

### Boundary Freeze Notes — `coordinator.rs`

Logical sections identified inside the 1739-line file:

| Lines | Content |
| --- | --- |
| 1–27 | Imports |
| 28–36 | `SESSION_TITLE_PROMPT` constant |
| 39–68 | `SessionTitleGenerator` struct + impl (`new`, `generate`) |
| 70–113 | Standalone helpers: `validate_title`, `deterministic_title`, `validate_or_fallback_title`, `extract_first_user_text` |
| 115–387 | `SessionManager` struct + impl (session CRUD, cache, title update, turn recording) |
| 389–528 | `CoordinatorImpl` struct + impl `CoordinatorImpl` (new, builder methods, drain/flush helpers, `agents()`) |
| 530–544 | `impl Coordinator for CoordinatorImpl` |
| 546–745 | `impl RequestHandler for CoordinatorImpl` (`handle`, `handle_stream`, `cancel_active_turn`) |
| 747–851 | Standalone `map_orchestrator_event` function |
| 853–1009 | `impl SessionService for CoordinatorImpl` |
| 1011–1739 | `#[cfg(test)] mod tests` |

Target split for `coordinator/`:

```
mod.rs              ← thin facade: mod declarations + pub use
session_title.rs    ← SessionTitleGenerator, validate_title, deterministic_title,
                       validate_or_fallback_title, extract_first_user_text
session_manager.rs  ← SessionManager struct + impl
impl.rs             ← CoordinatorImpl struct + impl CoordinatorImpl + impl Coordinator
request_handler.rs  ← impl RequestHandler for CoordinatorImpl + map_orchestrator_event
session_service.rs  ← impl SessionService for CoordinatorImpl
tests.rs            ← #[cfg(test)] mod tests
```

### Dependency Direction Constraints

- `session_title.rs` → only stdlib, `talos_llm_provider`, `talos_types`; no dependency on session management
- `session_manager.rs` → depends on `session_title.rs` (owns `SessionTitleGenerator` arc); no dependency on `impl.rs`
- `impl.rs` → depends on `session_manager.rs`; no dependency on `request_handler.rs` or `session_service.rs`
- `request_handler.rs` → depends on `impl.rs` and `session_manager.rs`; must not create new cross-crate dependency edges back toward `talos-agent`
- `session_service.rs` → depends on `impl.rs` and `session_manager.rs`
- `event_stream.rs` / `map_orchestrator_event` → lives in `request_handler.rs`, stays `pub(crate)` if not currently exported

### Execution Constraints For Later Phases

- Move `SessionTitleGenerator` and title helper functions first (pure, no dependencies) before touching `SessionManager`.
- Keep `MockAgent` isolated in `mock.rs`; it must not influence compilation of production runtime code.
- Run crate tests after each submodule migration, not only after completing the full file split.
- Preserve all existing `impl AgentImpl` method signatures; visibility changes are only allowed when the compiler proves wider visibility is required.

---

## 阶段 2: Split `agent.rs` Into Runtime Domains
**目标**: Move `agent.rs` into `agent/{mod,types,builder,kernel_bridge,orchestrator,tool_submission,mock,tests}.rs`, separating construction, kernel bridging, orchestrator protocol, and test doubles.
**成功标准**: `agent.rs` no longer exists as a monolithic file, the new `agent/mod.rs` acts as facade only, `MockAgent` lives in `mock.rs` exclusively, and the existing `pub use` surface from crate `lib.rs` is unchanged.
**测试**: After each moved submodule run `cargo test -p talos-agent`; after completing this phase run `cargo test -p talos-agent -- agent`
**状态**: 已完成

### Recommended Migration Order

1. Create `agent/types.rs` — move `KernelExecutionMode` and `ActiveTurnHandle` (no inbound deps from production code beyond the same file)
2. Create `agent/builder.rs` — move `AgentImpl` struct def + `impl AgentImpl` construction block + dead-code helpers
3. Create `agent/kernel_bridge.rs` — move kernel agent factory methods + `map_kernel_stream_event` + `map_thinking_source/stage`
4. Create `agent/tool_submission.rs` — move `validate_dynamic_tool_submission` + approval/dynamic-tool methods
5. Create `agent/orchestrator.rs` — move `impl Agent` + `impl AgentOrchestrator` + helper fns
6. Create `agent/mock.rs` — move `MockAgent` and all its trait impls
7. Create `agent/tests.rs` — move `#[cfg(test)] mod tests`
8. Replace `agent.rs` with `agent/mod.rs` thin facade

---

## 阶段 3: Split `coordinator.rs` Into Protocol Domains
**目标**: Move `coordinator.rs` into `coordinator/{mod,session_title,session_manager,impl,request_handler,session_service,tests}.rs`, separating session orchestration from protocol trait implementations.
**成功标准**: `CoordinatorImpl` remains the owning facade, `SessionManager` becomes its own module, `map_orchestrator_event` stays crate-internal in `request_handler.rs`, and no new dependency edge points from gateway back into coordinator.
**测试**: After each moved submodule run `cargo test -p talos-coordinator`; after completing this phase run `cargo test -p talos-coordinator -- coordinator`
**状态**: 已完成

### Recommended Migration Order

1. Create `coordinator/session_title.rs` — move `SESSION_TITLE_PROMPT`, `SessionTitleGenerator`, and standalone title helper fns
2. Create `coordinator/session_manager.rs` — move `SessionManager` struct + impl
3. Create `coordinator/impl.rs` — move `CoordinatorImpl` struct + `impl CoordinatorImpl` + `impl Coordinator`
4. Create `coordinator/request_handler.rs` — move `impl RequestHandler` + `map_orchestrator_event`
5. Create `coordinator/session_service.rs` — move `impl SessionService`
6. Create `coordinator/tests.rs` — move `#[cfg(test)] mod tests`
7. Replace `coordinator.rs` with `coordinator/mod.rs` thin facade

---

## 阶段 4: Reconcile Cross-Crate Use Sites And Cleanup
**目标**: Normalize re-exports, update tests, and remove transitional glue after both orchestration crates are split.
**成功标准**: Public surfaces remain stable, no temporary compatibility wrappers remain, and orchestration event flow still passes through the same runtime path as before.
**测试**: `cargo test -p talos-agent`, `cargo test -p talos-coordinator`, and `cd talos-rs && tests/integration/run_e2e.sh`
**状态**: 已完成

### 验收结果 (2026-04-12)

- `cargo test -p talos-agent`: 16 unit + 2 integration = **18 passed, 0 failed**
- `cargo test -p talos-coordinator`: **9 passed, 0 failed**
- `tests/integration/run_e2e.sh`: **8/8 PASS** (01–08, including memory flush, TUI quit)
