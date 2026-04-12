# 阶段 6：Final Integration Regression And Cleanup

> 本文件对应 `IMPLEMENTATION_PLAN.md` 中的主线阶段 6。
> 文件名中的 `05` 仅表示它是 Rust large-file split 主线的第 5 个子计划文件，不表示当前主线阶段编号。
> 前置条件是阶段 1-5 已完成；本阶段负责补齐残留超限文件、清理拆分遗留层，并完成最终全量回归与文件尺寸审计。

## 0. 当前基线

截至当前主线状态，已完成的拆分批次包括：

- 阶段 1：Roadmap Freeze
- 阶段 2：`agent-kernel` Split Batch
- 阶段 3：`agent` And `coordinator` Split Batch
- 阶段 4：`config` And `gateway/http` Split Batch
- 阶段 5：`prompt`, `memory`, `types`, And Provider Split Batch

当前 facade 层整体已经收瘦，但原始超限清单中仍有 2 个残留生产文件未降到阈值内：

- `talos-rs/crates/orchestration/agent/src/agent.rs`: `1832`
- `talos-rs/crates/orchestration/coordinator/src/coordinator.rs`: `1739`

这意味着本阶段不能只做 audit 和 regression，还必须先完成这两个残留文件的收口拆分，否则最终 file-size audit 无法通过。

## 阶段 1: Residual Oversized File Closeout
**目标**: Close the remaining oversized production files from the original split inventory without reopening already-stabilized module boundaries.
**成功标准**:
- `crates/orchestration/agent/src/agent.rs` and `crates/orchestration/coordinator/src/coordinator.rs` are both reduced below `1000` lines.
- Tests that still live inline in these files are moved to dedicated test modules where appropriate.
- Extracted modules follow existing responsibility boundaries instead of introducing `misc.rs` / `helpers.rs` style catch-all files.
- Public API paths remain stable for downstream crates.
**推荐拆分顺序**:
1. First move inline `mod tests` blocks out of `agent.rs` and `coordinator.rs`.
2. Then extract low-risk, single-direction responsibilities:
   - `agent.rs`: kernel event mapping, memory/kernel construction helpers, request execution helpers.
   - `coordinator.rs`: session resolution / creation helpers, stream handling helpers, approval / flush coordination helpers.
3. Only after the file sizes are under control, do a small facade cleanup pass to keep the crate roots thin.
**测试**:
- `cargo test -p talos-agent`
- `cargo test -p talos-coordinator`
**状态**: 已完成

## 阶段 2: Facade Audit
**目标**: Verify that every split subsystem now has a thin facade and no oversized compatibility hub has reappeared under a different filename.
**成功标准**:
- `lib.rs` or `mod.rs` files across split subsystems are limited to module declarations, re-exports, and thin entrypoints.
- No facade file has silently become the new home for core orchestration logic.
- Newly extracted modules from phase 1 do not simply relocate large mixed-responsibility code into another oversized file.
**测试**:
- `find talos-rs/crates \\( -name 'lib.rs' -o -name 'mod.rs' \\) -print0 | xargs -0 wc -l | sort -rn | head -30`
- Targeted crate tests for touched subsystems
**状态**: 已完成

## 阶段 3: Dead Code And Transitional Shim Cleanup
**目标**: Remove temporary aliases, compatibility wrappers, and no-longer-used helper modules left over from the split program.
**成功标准**:
- No dead transitional code remains from the large-file split program.
- Compatibility layers that still exist are explicitly justified as stable API boundaries rather than accidental split leftovers.
- No duplicate ownership of the same responsibility remains across old and new module trees.
**清理判定规则**:
- Can be removed:
  - Temporary aliases created only to bridge an in-flight file move
  - Split-time wrappers that do not own real behavior
  - Duplicate helper modules whose responsibility has already moved elsewhere
- Must be reviewed before removal:
  - Backward-compatibility re-exports that are part of a real crate boundary
  - Business-level `compat` semantics such as provider/model compatibility settings
  - Persistence / schema migration logic marked `legacy` but still needed for runtime upgrade safety
**重点审计对象**:
- `talos-rs/crates/gateway/core/src/types.rs`
- Any newly discovered split-era alias or wrapper introduced during earlier phases
**测试**: `cargo test` for each touched crate immediately after cleanup
**状态**: 已完成

## 阶段 4: Full Regression Sweep
**目标**: Run the full regression suite needed to trust the completed split program across crate tests and end-to-end scenarios.
**成功标准**:
- All relevant crate tests pass after the residual oversized files and shim cleanup land.
- The full `tests/integration/run_e2e.sh` regression passes after final cleanup.
- No regression appears in agent/coordinator orchestration behavior after the last split adjustments.
**测试**:
- `cd talos-rs && cargo test --workspace`
- `cd talos-rs && tests/integration/run_e2e.sh`
**状态**: 已完成

## 阶段 5: Final File-Size Audit
**目标**: Recount Rust source file sizes and confirm the split program actually removed all targeted oversized files without creating new ones elsewhere.
**成功标准**:
- The original oversized-file set from `spec/rust-large-file-split-plan.md` is fully eliminated.
- `agent.rs` and `coordinator.rs` are no longer above `1000` lines.
- No new oversized production file has appeared elsewhere as a side effect of closeout.
- If any remaining file is still above the threshold, it must be explicitly justified in review with a concrete reason; default expectation for this phase is zero unresolved leftovers from the original 13-file list.
**测试**:
- Re-run the repository-wide Rust file line count audit used by `spec/rust-large-file-split-plan.md`
- Compare the result directly against the original 13-file oversized inventory
**状态**: 已完成

## 最终验收矩阵

- 结构门禁
  - `agent.rs` and `coordinator.rs` both fall below `1000` lines
  - Facade files stay thin and do not re-accumulate orchestration logic
  - No new catch-all files such as `misc.rs` / `helpers.rs` are introduced
- 清理门禁
  - Split-era wrappers, aliases, and duplicate helpers are removed or explicitly justified
  - Stable compatibility boundaries are preserved; business compatibility logic is not mistakenly deleted
- 验证门禁
  - `cargo test -p talos-agent`
  - `cargo test -p talos-coordinator`
  - `cargo test --workspace`
  - `tests/integration/run_e2e.sh`
- 收口门禁
  - The original oversized-file inventory is fully closed out
  - `IMPLEMENTATION_PLAN.md` phase 6 can be marked `已完成` only after all gates above pass
