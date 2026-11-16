# Subagent Turn admission

Status: implemented

[Issue #150](https://github.com/apilots/talos/issues/150) tracks recursive
delegation that occupied unattended runs before they produced a deliverable.
This change extends the admission boundary of the
[Session Subagent design](../archive/2026/feat-032-session-subagent/design.md#6-execution).
Child Sessions retain their existing Agent, Turn history, and tool protocol.

## Admission policy

Configuration is scoped to the Talos runtime. Each primary Session tree has
its own active child count.

| Configuration | Default | Rule |
| --- | ---: | --- |
| `agent.subagent_admission.max_depth` | 3 | Count child levels below the primary Session. A value of `1` allows direct children only. Subagent Service checks depth before creating a child Session. The canonical Agent path limit still applies. |
| `agent.subagent_admission.max_active_turns` | 20 | The Turn Store refuses a child's first Turn when this many child Turns are already active in its tree. It checks capacity in the same transaction that records the new active Turn. |

Running, waiting, and cancelling child Turns count toward the new-spawn
threshold until their canonical settlement. A child with a previously settled
Turn can start a fresh `followup_task` at or above that threshold. The Store
checks canonical terminal events for this exception. The new follow-up counts
against later spawns and requires the child to have no current active Turn.
The primary Turn uses no child slot.

Depth or capacity refusal surfaces as `SessionBudgetExceeded`. The Worker
permit is consumed only after durable Turn admission. If a new child's
admission fails, the owning submission task cleans up its Session even when
the caller drops the request. An unresolved cancellation retains its active
record until the existing interrupt path confirms cleanup.

## Verification

- Exercise concurrent first starts, independent root trees, cancellation,
  and slot release with the Service and Store tests. Fill one tree, continue a
  previously settled child, and verify that the resulting active Turn blocks a
  new child while the tree remains at capacity.
- Run `just e2e-case 05_cli/exec_unattended` to exercise default depth, explicit
  refusal at the fourth child level, and delivery through all three admitted
  child levels.
- Run the pinned DeepSWE 1.1 `python-statemachine-state-data-scoping` task on
  native ARM64 with the shipped defaults. Retain the admitted tree and Turn
  outcomes separately from the unmodified verifier's score.

Worker finalization and OpenAI Responses stream termination are separately
tracked in [#155](https://github.com/apilots/talos/issues/155) and
[#156](https://github.com/apilots/talos/issues/156).
