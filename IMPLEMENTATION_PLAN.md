## 阶段 1: Establish Execution Framework
**目标**: Introduce the new execution framework skeleton in `agent-kernel`, including flow, processor, and event abstractions aligned with the approved design.
**成功标准**: The crate exposes compile-safe `SingleFlow`, `AutoFlow`, processor traits, and execution event types without breaking current behavior.
**测试**: `cargo test -p talos-agent-kernel`
**状态**: 已完成

## 阶段 2: Route Existing Main Path Through SingleFlow
**目标**: Reuse the current general turn execution via `SingleFlow` so the framework becomes the active baseline path.
**成功标准**: `LlmKernelAgent` no longer calls `run_general_turn()` directly; `SingleFlow` becomes the shared execution entry.
**测试**: `cargo test -p talos-agent-kernel`
**状态**: 已完成

## 阶段 3: Introduce Processor Pipelines
**目标**: Add request/response processor pipelines and connect them to the flow lifecycle while keeping behavior equivalent to the current path.
**成功标准**: Pipelines exist as first-class abstractions with no-op defaults and are invoked by `SingleFlow`.
**测试**: `cargo test -p talos-agent-kernel`
**状态**: 已完成

## 阶段 4: Align Session Transcript Semantics
**目标**: Add transcript event projection and make the flow own per-event execution facts while preserving current session summary/shared-memory behavior.
**成功标准**: Execution events are projected from the baseline path and available for later transcript integration without changing current persistence semantics incorrectly.
**测试**: `cargo test -p talos-agent-kernel`
**状态**: 已完成

## 阶段 5: Expand Main Flow Features
**目标**: Gradually replace direct general-turn internals with flow-driven loop details and prepare built-in/auto-flow extension points.
**成功标准**: `SingleFlow` owns typed step execution, step-level event generation, and tool-loop orchestration; remaining work stays within the new framework for callbacks, plugins, parallel tools, and transfer behavior.
**测试**: `cargo test -p talos-agent-kernel` and `tests/integration/run_e2e.sh`
**状态**: 已完成

## 阶段 6: Restore Cross-Turn Session Replay
**目标**: Fix transcript loading semantics so a new invocation can replay the full session history for the active branch.
**成功标准**: Loading a session without an invocation filter returns all prior transcript events for that session and branch.
**测试**: `cargo test -p talos-agent-kernel session`
**状态**: 已完成

## 阶段 7: Stabilize Parallel Tool Ordering
**目标**: Keep parallel tool execution results in the same order as the originating assistant `tool_calls`.
**成功标准**: Parallel execution remains concurrent while returned tool outputs are deterministic and index-aligned with input calls.
**测试**: `cargo test -p talos-agent-kernel tool_execution`
**状态**: 已完成

## 阶段 8: Preserve Retrieved Memories Across Rounds
**目标**: Make flow step requests reuse accumulated loop state instead of overwriting retrieved-memory system messages before the next model call.
**成功标准**: Retrieved memories injected into `state.messages` are present in the following round request.
**测试**: `cargo test -p talos-agent-kernel flow`
**状态**: 已完成
