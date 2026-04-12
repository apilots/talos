## 阶段 1: Baseline And Boundary Freeze
**目标**: Freeze the current `agent-kernel` public surface and confirm the concrete split boundaries for `general_turn`, `flow`, and `processor` before moving code.
**成功标准**: The current exports in `talos-rs/crates/orchestration/agent-kernel/src/lib.rs` are inventoried, the target directory layout is fixed, and the dependency constraints are explicitly documented for implementation work.
**测试**: `cargo test -p talos-agent-kernel`
**状态**: 已完成

### Baseline Line Counts

Current oversized file sizes before any split work:

- `general_turn.rs`: `2180`
- `flow.rs`: `2013`
- `processor.rs`: `1840`

### Frozen Export Surface

Current `agent-kernel/src/lib.rs` keeps a flat facade and re-exports the following split targets:

- `general_turn`: `apply_general_turn_step`, `apply_general_turn_tool_batch`, `build_general_turn_step_request`, `emit_general_turn_finalization_stream`, `execute_general_turn_loop`, `execute_general_turn_step`, `execute_general_turn_tool_batch`, `finalize_general_turn_artifacts`, `flush_session_memory`, `persist_general_turn_artifacts`, `prepare_general_turn`, `run_general_turn`, plus all exported turn state/result types
- `flow`: `AutoFlow`, `BaseLlmFlow`, `FlowExecution`, `FlowFactory`, `FlowRunSummary`, `SingleFlow`
- `processor`: `RequestProcessor`, `ResponseProcessor`, both `*Kind` enums, both pipelines, and all currently public request/response processor structs

The implementation requirement for later phases is to preserve this crate-level re-export surface unless a dedicated cleanup phase explicitly removes or renames an item.

### Boundary Freeze Notes

- `general_turn.rs`
  Types appear near the top of the file.
  `build_general_turn_step_request()` starts around `1454`.
  `execute_general_turn_step()` starts around `1469`.
  `execute_general_turn_tool_batch()` starts around `1680`.
  `prepare_general_turn()` starts around `1909`.
  This confirms the `types / step_request / step_execute / tool_batch / finalize / prepare` split shape is aligned with the current code.
- `flow.rs`
  The stable public seam is `types + flow implementations + factory + callbacks`.
  `FlowFactory` remains the critical assembly point and must not be mixed back into concrete flow implementation files after splitting.
- `processor.rs`
  The stable public seam is `request pipeline + response pipeline + grouped processor implementations`.
  `planning.rs` must remain one-way dependent on processor-local traits/output types plus shared prompt/types only.
  It must not depend on concrete `flow` or `general_turn` implementations.

### Execution Constraints For Later Phases

- Move shared types and pure helper logic before moving runtime orchestration code.
- Keep new modules internal-first and widen visibility only when the compiler proves it is required.
- Run crate tests after each submodule migration, not only after each phase.

## 阶段 2: Split `general_turn` Into Directory Modules
**目标**: Move `general_turn.rs` into `general_turn/{mod,types,prepare,step_request,step_execute,tool_batch,finalize,memory_summary,tests}.rs` without changing runtime behavior.
**成功标准**: `general_turn.rs` no longer exists as a monolithic file, the new `general_turn/mod.rs` acts as facade only, and moved APIs preserve the existing `pub use` surface from crate `lib.rs`.
**测试**: After each moved submodule run `cargo test -p talos-agent-kernel`; after completing this phase run `cargo test -p talos-agent-kernel -- general_turn`
**状态**: 已完成

### Split Result Snapshot

`general_turn.rs` has been replaced by `general_turn/` directory modules:

- `mod.rs`: `86`
- `types.rs`: `188`
- `memory_summary.rs`: `194`
- `step_request.rs`: `16`
- `step_execute.rs`: `290`
- `tool_batch.rs`: `350`
- `finalize.rs`: `96`
- `prepare.rs`: `306`
- `tests.rs`: `697`

The runtime modules are all below the project threshold after the split, and `mod.rs` remains a thin facade plus shared imports/constants. Test helpers and assertions stay in `tests.rs`, which is also below the threshold.

### Validation Notes

- `cargo test -p talos-agent-kernel` passed after the split.
- `cargo test -p talos-agent-kernel -- general_turn` passed after the split.
- The `general_turn`-scoped unit tests remained green under the crate-wide run:
  - retrieval-memory parsing tests
  - streaming tool-call id normalization test
  - memory flush tests
  - pending-tool / prompt-warning / exec-loop runtime tests

## 阶段 3: Split `flow` With Factory And Callback Isolation
**目标**: Move `flow.rs` into `flow/{mod,types,single,auto,factory,callbacks,tests}.rs`, separating flow implementations, factory wiring, and callback helpers.
**成功标准**: `FlowFactory`, `SingleFlow`, `AutoFlow`, and callback-related code live in separate files, with no new circular dependencies and no behavior changes.
**测试**: After each moved submodule run `cargo test -p talos-agent-kernel`; after completing this phase run `cargo test -p talos-agent-kernel -- flow`
**状态**: 已完成

### Split Result Snapshot

`flow.rs` has been replaced by `flow/` directory modules:

- `mod.rs`: `53`
- `types.rs`: `36`
- `single.rs`: `740`
- `callbacks.rs`: `71`
- `factory.rs`: `200`
- `auto.rs`: `36`
- `tests.rs`: `901`

The runtime modules are all below the project threshold after the split. `single.rs` remains the largest file because it carries the full `run_events()` orchestration loop, but it is now isolated from profile selection and callback helpers.

### Validation Notes

- `cargo test -p talos-agent-kernel` passed after the split.
- `cargo test -p talos-agent-kernel -- flow` passed after the split.
- `flow`-scoped checks remained green for:
  - profile selection and branch mapping
  - request/response envelope helper behavior
  - PlanReAct replan and local-path probe policies
  - callback stop behavior and confirmation staging

## 阶段 4: Split `processor` With Directional Dependency Rules
**目标**: Move `processor.rs` into `processor/{mod,request,response,request_kinds,response_kinds,identity,compaction,contents,planning,tests}.rs` while preserving the existing request/response pipeline behavior.
**成功标准**: Request and response processor pipelines are separated, `planning.rs` depends only on processor-local traits/output types plus shared prompt/types, and it does not depend on `flow` or `general_turn` concrete implementations.
**测试**: After each moved submodule run `cargo test -p talos-agent-kernel`; after completing this phase run `cargo test -p talos-agent-kernel -- processor`
**状态**: 已完成

### Split Result Snapshot

`processor.rs` has been replaced by `processor/` directory modules:

- `mod.rs`: `46`
- `request.rs`: `110`
- `response.rs`: `77`
- `request_kinds.rs`: `424`
- `response_kinds.rs`: `10`
- `identity.rs`: `45`
- `compaction.rs`: `186`
- `contents.rs`: `28`
- `planning.rs`: `106`
- `response_processors.rs`: `200`
- `tests.rs`: `647`

The request/response traits and pipelines are now isolated from concrete processor implementations, and `planning.rs` remains limited to planner-driven request/response effects plus shared execution types.

### Validation Notes

- `cargo test -p talos-agent-kernel` passed after the split.
- `cargo test -p talos-agent-kernel -- processor` passed after the split.
- `processor`-scoped checks remained green for:
  - planning request/response effects
  - final answer and tool-call projection
  - compaction boundaries and current-invocation exclusion
  - confirmation replay and transfer instruction staging

## 阶段 5: Consolidate Exports, Tests, And Cleanup
**目标**: Reconcile `agent-kernel/src/lib.rs` re-exports, move tests to the new module-local locations, and remove leftover transitional code after the three split phases.
**成功标准**: `lib.rs` remains a thin facade, no temporary compatibility shims or dead modules remain, and the three split subsystems are all below the project file-size threshold.
**测试**: `cargo test -p talos-agent-kernel` and `cd talos-rs && tests/integration/run_e2e.sh`
**状态**: 已完成

### Cleanup And Validation Notes

- `agent-kernel/src/lib.rs` continues to act as a flat facade over `general_turn/`, `flow/`, and `processor/`.
- No compatibility shim modules were added during the split; the crate now resolves directly through directory modules.
- All three oversized files are fully replaced by submodules whose runtime files remain below the threshold.
- Final validation passed:
  - `cargo test -p talos-agent-kernel`
  - `cd talos-rs && tests/integration/run_e2e.sh`
- Full integration regression passed through scenario `08_embedded_tui_quit_flushes_memory.sh`.
- E2E report:
  - `talos-rs/tests/integration/reports/e2e_report_20260412_173015.md`
