# Talos Tool Contract Status Reconciliation And Spec Next Steps

## 1. Purpose

This document turns the latest research conclusion into a design-side spec artifact.

It does **not** enter implementation. Its purpose is to:

1. reconcile the current Talos implementation state against the existing `spec/tools` documents
2. compare the current shape with Claude Code's tool contract
3. distinguish clearly between:
   - already landed behavior
   - partially landed behavior
   - target-state-only behavior
4. define the next spec-level closure path as `P0 / P1 / P2`

Short answer:

**Talos already has a usable base for tool schema exposure, result shaping, failure hints, summary carry-forward, and request-time tool selection, but it has not yet converged into Claude Code's thicker unified tool contract and explicit failure-loop protocol.**

---

## 2. Scope

This document focuses only on:

- tool input contract
- tool output contract
- failure semantics and retry guidance
- summary / transcript / next-turn carry-forward
- request-time tool exposure status

This document does **not** define:

- direct implementation patches
- per-tool runtime rewrites
- UI pixel-level behavior
- final test script details beyond acceptance gates

---

## 3. Current Codebase Findings

## 3.1 Input Contract: Partially Landed

Talos already has:

- per-tool `parameters_schema()`
- tool registry metadata
- request-time prompt exposure based on selected tool definitions

Relevant code:

- `talos-rs/crates/action/tools/src/trait.rs`
- `talos-rs/crates/action/tools/src/tools/read.rs`
- `talos-rs/crates/action/tools/src/tools/edit.rs`
- `talos-rs/crates/action/tools/src/registry.rs`

The current Talos `Tool` trait already includes multiple contract-shaping slots such as:

- `internal_name`
- `aliases`
- `user_facing_name`
- `source`
- `payload_kind`
- `visibility`
- `parallelism`
- `mutating`
- `behavior_flags`
- `result_policy`

So the gap is not simply "too few trait methods".

The more precise gap is:

- Talos still lacks the two semantic gate slots `validate_input` and `check_permissions`
- argument decoding is still mostly performed inside each `execute()` body via `serde_json::from_value(...)` or `decode_tool_input(...)`

So Talos currently has **schema presence**, but it does **not** yet have a single unified contract layer equivalent to Claude Code's:

- schema-first parse
- semantic `validateInput`
- tool-level `checkPermissions`
- unified execution gating before runtime body execution

Status:

**Input contract exists, but validation and permission semantics are not yet centralized into a first-class platform contract.**

## 3.2 Output Contract: Partially Structured, Still String-Centric

Talos currently uses:

- `ToolOutput { success, output: String, error }` as the base local-tool result
- `ToolOutputPayload` and `ToolDispatchOutcome` as the higher-level dispatch wrapper

Relevant code:

- `talos-rs/crates/action/tools/src/trait.rs`
- `talos-rs/crates/action/tools/src/output.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/tool_execution.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/tool_batch.rs`

Important detail:

- `ToolOutput.output: String` is the first-hand tool-layer return path
- some tools serialize structured results back into strings before returning
- `ToolOutputPayload::Function.structured_content` is frequently reconstructed later via `From<ToolOutput>` and `serde_json::from_str(...)` rather than being the primary native result envelope

Status:

**Result shaping has started, but the main success path is still too string-centric to be considered fully converged.**

## 3.3 Failure Semantics: Present But Heuristic

Talos already defines:

- `ToolFailureKind`
- progress / rejected / error / summary rendering helpers

Relevant code:

- `talos-rs/crates/action/tools/src/output.rs`

Current supported failure kinds include:

- `validation_failure`
- `permission_rejection`
- `governance_rejection`
- `stale_write_failure`
- `deferred_tool_unavailable`
- `runtime_failure`

This is meaningful progress.

However, classification is still largely inferred from error strings such as:

- `invalid`
- `permission denied`
- `stale`
- `deferred`

And more importantly:

- `ToolFailureKind` is a first-class enum
- but `retryable` and `suggested_action` are **not** first-class tool result fields today
- instead, they are derived inside `From<ToolOutput> for ToolOutputPayload`
- that derivation uses `classify_error_kind(...)` plus `suggested_action_for_error(...)`
- the derived metadata is then serialized into `structured_content`

Status:

**Talos already exposes failure guidance to the model, but the path is not yet strongly typed end-to-end and still depends too much on textual heuristics.**

## 3.4 Summary / Transcript / Next-Turn Carry-Forward: Baseline Exists

Talos already provides:

- `summarize_for_model()`
- `summarize_for_transcript()`
- tool result stream emission
- final execution summary in the general-turn flow

Relevant code:

- `talos-rs/crates/action/tools/src/output.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/tool_batch.rs`
- `talos-rs/crates/orchestration/agent-kernel/src/general_turn/finalize.rs`

So the answer to "is summary support missing?" is **no**.

The real gap is different:

- summary semantics are not yet fully unified with the tool contract
- progress / rejected / error / summary are not yet equivalent first-class outputs in the same way Claude Code's tool interface expects

Status:

**Summary and transcript carry-forward exist, but they are still platform-mapped helpers rather than a fully unified contract surface.**

More precisely:

- the current closure path is already implemented across multiple runtime points
- but it is not yet driven by one shared canonical envelope and one shared semantic event schema
- so the problem is not absence, but fragmentation

Current responsibility split should be understood as:

- `tool_execution.rs`
  - executes resolved tool calls
  - constructs per-call execution outputs
  - normalizes execution failures into dispatch outcomes
- `general_turn/tool_batch.rs`
  - reduces tool outputs into model-visible payloads
  - emits tool result stream events
  - prepares summary/carry-forward material for the general turn

This distinction matters because the current gap is not "no closure path", but "closure responsibilities remain distributed across execution and reduction layers instead of being sourced from one canonical result contract".

## 3.5 Request-Time Tool Selection: Partially Landed, Not Absent

Older spec language says request-time tool selection is "not landed". That is no longer fully accurate.

Talos already has:

- `ToolSelectionRequest`
- deterministic request-time selection in `registry.rs`
- `active / deferred / always_loaded` selection output
- prompt assembly based on selected tools rather than blindly dumping the full catalog

Relevant code:

- `talos-rs/crates/action/tools/src/registry.rs`
- `talos-rs/crates/common/prompt/src/assembler.rs`

Current selector inputs include:

- user input text
- permission mode
- deferred activation state
- visibility flags

Existing narrowing already includes keyword-driven conditional activation for several domains, including:

- Web / URL / search shaped requests
- memory / recall shaped requests
- agent / delegate shaped requests
- todo / plan shaped requests

What is still missing is the **full Claude Code-grade narrowing layer**, including richer context-driven capability routing and thicker compile-time control over the visible tool surface.

Status:

**Request-time selection is partially landed. It is no longer accurate to describe it as entirely unimplemented.**

For avoidance of doubt:

**partially landed** here means:

- usable in the current product path
- already influencing prompt-visible tool exposure
- but still below the final contract/strategy shape required for production-grade capability narrowing

---

## 4. Claude Code Comparison

Claude Code's tool contract is structurally thicker.

Relevant code:

- `claude-code/src/Tool.ts`
- `claude-code/src/services/tools/toolExecution.ts`
- `claude-code/src/utils/hooks/hooksConfigManager.ts`

Claude Code standardizes all of the following as first-class contract or execution-path concepts:

| Slot | Claude Code status | Talos status | Closure phase |
| --- | --- | --- | --- |
| input schema | first-class | present | already landed |
| validation slot | first-class `validateInput` | missing as a first-class trait slot | `P0` |
| permission slot | first-class `checkPermissions` | outer allow-list exists, per-tool semantic gate missing | `P0` |
| visibility / defer slot | first-class | present | already landed / partially landed |
| read-only / concurrency semantics | first-class | present via `behavior_flags` and `parallelism` | already landed |
| summary slot | first-class `getToolUseSummary` | partial, platform-side summary helpers exist | `P1` |
| progress render slot | first-class `renderToolUseProgressMessage` | partial platform mapping exists | `P2` |
| rejected render slot | first-class `renderToolUseRejectedMessage` | partial platform mapping exists | `P2` |
| error render slot | first-class `renderToolUseErrorMessage` | partial platform mapping exists | `P2` |
| structured result return | first-class | partial, often reconstructed from string output | `P1` |
| hook-driven retry / denial surface | first-class | not yet explicit end-to-end | `P2` |

Claude Code's main advantage is therefore not "more tools", but:

1. a thicker tool contract
2. a more standardized execution gate
3. a more explicit failure-to-next-turn bridge
4. more mature model-visible result semantics

Talos is already moving in this direction, but has not yet fully closed that gap.

---

## 5. Judgment Against Existing Talos Specs

## 5.1 What Remains Accurate

The following existing direction remains correct:

- Talos should converge toward a thicker tool platform contract
- result semantics still need hardening
- failure-to-retry guidance is still weaker than target state
- Claude Code alignment should be platform-first, not tool-by-tool patchwork

Relevant specs:

- `spec/tools/01-tool-platform-architecture-alignment.md`
- `spec/tools/11-tool-surface-selection-and-contract-hardening.md`
- `spec/tools/implementation-plan/09-tool-contract-and-result-hardening.md`

## 5.2 What Is Now Stale

The following older wording is now partially stale:

- describing request-time tool selection as completely unlanded
- implying Talos still relies purely on "full exposure + prompt guidance" without request-time filtering

The more accurate wording should be:

- request-time selection is **partially landed**
- deferred tool handling is **partially functional**
- tool result hardening is **in progress but not converged**

## 5.3 Final Design-Side Judgment

Talos is **not** off-track.

The correct judgment is:

**The direction is consistent with the documented architecture, but the implementation is still below the target-state contract shape.**

Or more plainly:

**This is not a direction problem. It is a convergence problem.**

---

## 6. Next Spec Closure Path

The next stage should remain design-led and should be described as three closure phases.

Dependency order:

```text
P0 input contract and execution gate
  -> defines canonical failure/error fields
  -> enables
P1 result envelope unification
  -> defines canonical result and summary source
  -> enables
P2 cross-turn failure-loop bridge
```

## P0: Unify Input Contract And Execution Gating

Goal:

- move from "schema exists" to "schema-first execution gate"
- stop scattering argument decoding and semantic validation across `execute()` bodies

Required design changes:

- extend the tool contract with `validate_input`
- extend the tool contract with `check_permissions`
- define a shared pre-execution gate in registry/orchestrator flow
- standardize validation failure output shape

Minimum structured failure fields produced by P0:

- `error_kind`
- `retryable`
- `suggested_action`
- `error_message`

Acceptance at spec level:

- validation errors have a stable structured shape
- outer allow-list policy gating remains in place before runtime tool body execution
- per-tool semantic `check_permissions` gating is added before runtime tool body execution
- tools no longer need to implement argument parsing as their main validation path

## P1: Introduce A Stable Tool Result Envelope

Goal:

- stop treating string output as the primary canonical result
- make structured result semantics the main path

Required design changes:

- introduce a stable `ToolResultEnvelope`
- separate:
  - raw runtime output
  - model-visible result
  - transcript / render summary
- preserve `raw_output` only as compatibility data, not as canonical model contract

`P1` must build on the error/failure field model introduced by `P0` rather than redefining failure metadata in a second layer.

Minimum envelope shape:

```rust
pub struct ToolResultEnvelope {
    pub status: ToolResultStatus,
    pub payload: Option<serde_json::Value>,
    pub error: Option<ToolResultError>,
    pub policy: ToolResultPolicyMeta,
    pub timing: Option<ToolTimingMeta>,
    pub raw_output: Option<String>,
}
```

```rust
pub enum ToolResultStatus {
    Success,
    Failure,
    Pending,
    Rejected,
}
```

```rust
pub struct ToolResultError {
    pub error_kind: String,
    pub error_message: String,
    pub retryable: bool,
    pub retry_policy_code: Option<String>,
    pub suggested_action: Option<String>,
    pub next_turn_payload: Option<serde_json::Value>,
}
```

```rust
pub struct ToolResultPolicyMeta {
    pub max_inline_bytes: Option<usize>,
    pub persisted_large_output: bool,
    pub preview_lines: Option<usize>,
}
```

Acceptance at spec level:

- success and failure both expose stable structured fields
- retry metadata is not reconstructed from string parsing as the primary path
- summary generation reads from the envelope, not from ad hoc string recovery

## P2: Make Failure Closure Explicit Across Turns

Goal:

- make failure handling more than "error text plus suggestion"
- define a formal bridge from tool failure into next-turn model decision-making

Required design changes:

- define stable retry policy codes such as:
  - `retry`
  - `fallback`
  - `reject`
- write failure strategy metadata into result/event/context flow
- standardize `tool_use_*` semantic events for summary and next-turn carry-forward

Minimum event set to standardize:

- `tool_use_start`
- `tool_use_progress`
- `tool_use_rejected`
- `tool_use_error`
- `tool_use_summary`
- `tool_use_complete`

Minimum strategy codes:

- `retry`
- `fallback`
- `reject`

Minimum cross-turn failure payload:

- `error_kind`
- `retryable`
- `retry_policy_code`
- `suggested_action`
- `next_turn_payload`

Illustrative mapping:

| Failure class | retryable | retry_policy_code | expected next step |
| --- | --- | --- | --- |
| validation | false | `reject` | fix arguments before retry |
| permission | false | `fallback` | request approval or pick lower-privilege tool |
| runtime | true | `retry` or `fallback` | narrow scope, retry, or switch tool |

Acceptance at spec level:

- validation, permission, and runtime failures map to explicit next-step strategy classes
- next-turn context receives structured failure guidance rather than only textual error payloads
- summary, transcript, and model-visible follow-up all read from the same strategy-aware result path
- `tool_use_*` events and model-visible result metadata must read from the same canonical field source rather than duplicated ad hoc text generation

---

## 7. Review Focus

When this spec is reviewed, review should focus on:

1. whether the current-state assessment is faithful to actual Talos code
2. whether the spec correctly distinguishes "already landed" from "target-state only"
3. whether `P0 / P1 / P2` are the right closure order
4. whether the failure semantics are concrete enough to guide future implementation
5. whether the updated wording avoids overstating the current gap

---

## 8. Final Statement

The final statement for this research topic is:

**Talos already has a usable base for tool schema exposure, result shaping, failure hints, summary carry-forward, and request-time tool selection. However, it has not yet converged into the stronger unified tool contract, standardized result envelope, and explicit failure-loop bridge that Claude Code already treats as first-class platform behavior.**

That is why the correct conclusion is:

**direction not off-track, target state not yet converged.**
