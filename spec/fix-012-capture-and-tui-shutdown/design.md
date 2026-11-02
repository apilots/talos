# Execution Metrics and terminal logging lifetime

Status: active

Parallel worker timing callbacks can lose a completed tool end on shared-map
contention. Tool tasks instead return optional ToolExecutionMetrics: queue/start/
end Unix milliseconds and monotonic queue/execution durations. The common execution
future measures entry through dispatch result, including preparation and handoff.
Carry the same object through ToolResultStreamData into existing AgentToolResult
history/events. Never put Metrics into model-facing tool content or another table.
Pending output ends this attempt; subsequent user waiting is not execution time.
A forced abort without output does not invent completion metrics.

LLM calls use independent LlmCallMetrics: start/end, execution duration, first/last
output, TTFT/generation and existing TokenUsage. Preserve source clock identity.
Remove tracing Interval DTOs and worker timing callbacks. UI consumes result Metrics
and LLM Metrics directly, preserving the existing ledger, overview and detail layout.

A versioned startup Upgrade converts known old LLM timing/usage and removes old
interval fields before canonical readers start. Historical tool Output never held
Metrics; leave those empty rather than rewrite already synchronized canonical
events. Preserve prompts, results, Session/Turn and event identities. No runtime
legacy reader, dual writes, new tables or per-tool implementation changes.

AgentToolResult.metrics changes Sync's closed business contract: regenerate the
canonical schema/SDK types and coordinate the Talos dependency. Full prompt/response
snapshots remain local as before. Review PRs do not authorize publication/deployment.

TUI also drops its logging guard before Tokio tears down background tasks. Keep
that guard in the synchronous executable host, declared before the runtime, so
runtime teardown logs finish before the appender drains. Do not suppress messages
or change SDK/background-task ownership.

Verification extends existing executor and controlled TUI scenarios, including
persisted Output Metrics and normal quit stderr. Add one focused real-SQL upgrade
case; preserve useful interaction coverage and remove obsolete interval assertions.
Authenticated Sync shutdown is checked separately from the secretless TUI fixture.
