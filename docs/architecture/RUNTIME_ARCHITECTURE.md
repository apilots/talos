# Agent Provider and durable execution architecture

[Chinese](RUNTIME_ARCHITECTURE_zh.md)

## Ownership chain

```text
TalosService
    |
    +-- owns product lifecycle and durable state transitions
    |
    v
Coordinator
    |
    +-- resolves Routed or Exact Agent bindings
    |
    v
AgentRegistry
    |
    +-- returns a registered AgentHarness
    |
    v
AgentHarness
    |
    +-- owns common live lifecycle, admission, cancellation, and event consistency
    |
    v
AgentProvider SPI
    |
    +-- creates one in-memory AgentSession per Talos Session
    |
    +-- KernelAgentProvider [current] -> KernelAgentSession -> Agent Kernel
    +-- CodexAgentProvider [planned]
    +-- ClaudeCodeAgentProvider [planned]
    `-- PiAgentProvider [planned]
    |
    v
Agent progress + unique settlement
    |
    v
Service -> Store commit -> Timeline publication -> SDK / client
```

`TalosService` is the product-facing facade. Protocol adapters call it instead
of constructing stores, Providers, or registries directly. The Agent Kernel
subsystem (`talos-agent-kernel`) is the private execution engine used by
`KernelAgentProvider`; it is not the universal implementation behind every
future Agent Provider.

## Multiple Agents and Agent Provider SPI

Multi-Agent registration and Provider implementation are separate dimensions:

```text
Agent configuration
    |
    +-- AgentDefinition: default  -> kind=kernel -> KernelAgentProvider
    +-- AgentDefinition: reviewer -> kind=kernel -> KernelAgentProvider
    +-- AgentDefinition: coder    -> kind=codex  -> CodexAgentProvider [planned]
    +-- AgentDefinition: analyst  -> kind=claude -> ClaudeCodeAgentProvider [planned]
    `-- AgentDefinition: pi       -> kind=pi     -> PiAgentProvider [planned]
    |
    v
AgentDefinitionLoader
    |
    v
Composition Root selects a ready AgentProvider from typed config
    |
    v
AgentRegistry stores multiple AgentHarness registrations by Agent id
    |
    v
Router / Coordinator resolves the initial Session binding
    |
    v
Later Turns dispatch with that Session's exact Agent binding
```

Multiple Agent definitions may use the same Provider implementation with
different identities, models, or typed Provider configuration. Workspace scope
is resolved independently from Workspace configuration and trusted User identity. A
new implementation targets `AgentProvider` plus `AgentSession` and remains
behind the common Harness lifecycle; Coordinator must not downcast or branch on
a concrete Provider kind.

This extension boundary is implemented and Provider-neutral. The Harness stores
only the SPI; Kernel-specific request, event, continuation, and transcript types
stay behind `KernelAgentProvider`. Current composition registers only the typed
Kernel Provider. Codex, Claude Code, and Pi are not exposed as selectable kinds
until their Provider implementations, typed configs, capabilities, and
composition-root branches exist; there is no fallback to Kernel.

## Kernel-backed Turn execution

The following flow is the built-in `KernelAgentProvider` path. Other Providers
may use different private backends, but must return the same standardized Agent
events, results, capabilities, cancellation behavior, and continuation behavior.

```text
Client             Service / Store             Harness / Kernel Provider
  |                       |                                |
  |-- Turn request ------>| validate + build AgentContext   |
  |                       |-- reserve execution ---------->|
  |                       |<-- admission + paused permit --|
  |                       |                                |
  |                       | commit admission + active      |
  |                       | + idempotency state             |
  |                       |-- consume permit ------------->|
  |<-- accepted ACK ------|                                |
  |                       |                 build bounded context
  |                       |                 from memory/history/Skills
  |                       |                                |
  |                       |                 +---- model call <-----+
  |                       |                 |                     |
  |                       |                 +-- text/reasoning     |
  |                       |                 |                     |
  |                       |                 +-- tool request      |
  |                       |                     policy/approval   |
  |                       |                     sandbox/execution |
  |                       |                     bounded result ---+
  |                       |                                |
  |                       |<-- progress -------------------|
  |<-- committed events --| persist, then publish           |
  |                       |<-- unique settlement ----------|
  |                       | commit waiting OR terminal     |
  |<-- committed fact ----| publish                         |
```

ACK confirms admission, not completion; events may race with ACK delivery.
Execution starts only after durable admission. Owned admission tasks prevent
request cancellation from orphaning an accepted operation. Progress delivery
excludes waiting/terminal facts; the separate channel carries the unique
settlement per invocation, including cumulative logical-Turn metrics. A missing
settlement is an explicit failure, not a synthesized successful completion.

## Settlement, state and recovery

```text
Committed admission --> Running -- waiting settlement --> WaitingForInput
                           |                                    |
                           |                          claim + same-Turn resume
                           |<-----------------------------------+
                           |
                           +-- completion/failure --> terminal commit
                           |
                           `-- cancellation --> cleanup outcome
                                                    |
                                  +-----------------+------------------+
                                  |                                    |
                              confirmed                            unconfirmed
                                  |                                    |
                            terminal commit                   retain active isolation
```

This is a lifecycle view, not an additional state machine. Store owns durable
active state; Service owns its live writer and product completion; Harness owns
tasks and cleanup. A waiting invocation releases live execution without ending
the logical Turn. Input reservations prevent settlement from overtaking an
uncommitted input. Terminal commits compare the expected durable tail.

`AgentRunSettlement::Finished` retains the original in-process `TalosResult<()>`
alongside public facts/messages/metrics. Report the original failure at its
handling boundary; never reconstruct diagnostics from a persisted `ApiError`.
History results are bounded projections of committed facts, not another stored
transcript or result JSON. Provider artifacts remain private execution material.

| Situation | Recovery contract |
| --- | --- |
| Browser refresh while execution is live | Replay through a captured high-water mark, then receive live events; do not resubmit input |
| Waiting with an unclaimed continuation | Keep the pending input available |
| Claimed continuation with no subsequent durable progress | Resume the same Turn from the supported continuation and committed response |
| Claimed continuation already advanced without a safe resume point | Do not replay old side effects; fail once after confirmed cleanup |
| Orphaned nonterminal execution without continuation | Recover from durable facts and fail after confirmed cleanup |
| Cleanup cannot be confirmed | Retain active isolation; do not fabricate completion |

Natural completion and cancellation converge through the durable terminal
commit boundary. Cancellation does not automatically dispatch queued work.
Neither arbitrary-position resume nor exactly-once external tool effects are
guaranteed. Provider `compact`, `clear` and `close` have distinct contracts; see
[Domain model](DOMAIN_MODEL.md).

## Product notices share the durable Timeline

```text
Agent progress / settlement             Service product-effect failure
            |                                        |
            v                                        v
      AgentEvent                              SystemNotice(ApiError)
      execution sequence                      no execution sequence
            |                                        |
            +----------------+-----------------------+
                             v
                   Store: one durable Timeline
                   stable event ID + global cursor
                             |
                    +--------+--------+
                    v                 v
              Session stream     History + notices

Only Agent facts --> Agent reducer / checkpoint / Provider continuation
```

Title generation and eligible memory reflection run before terminal commit;
their failures produce warnings without replacing the main execution result.
Other completion effects, including usage and queue handling, have their own
failure boundaries. Notices can therefore precede or follow the terminal fact.
Interactive clients retain Session subscriptions; Turn-limited subscriptions
end at the terminal fact. Durable publication resolves order across writers.
If notice persistence fails, report that failure separately without inventing a
replay cursor. See [Error contract](ERROR_MODEL.md) for report ownership and
[Client architecture](CLIENT_ARCHITECTURE.md) for replay/presentation rules.

## Subagent execution

Subagent collaboration is orthogonal to Agent Provider registration.

```text
Parent Session / Turn / Agent binding
                 |
                 v
Collaboration tool validates the parent context
                 |
                 v
Create Child Session and Child Turn
                 |
                 v
Inherit the parent's exact Agent binding
                 |
                 v
Re-enter Coordinator -> same AgentHarness -> same AgentProvider implementation
                 |
                 v
Child Timeline and result projection
```

A subagent does not silently select another Agent definition or bypass normal
Session and Turn lifecycle rules. Cross-Agent delegation, if introduced, must
be a separate explicit product capability.

## Tool capability flow

```text
Model tool call
      |
      v
Tool registry and deferred discovery
      |
      v
Permission policy and optional user interaction
      |
      +-- denied -------------------------> typed tool failure
      |
      v
Execution plan and sandbox selection
      |
      +-- built-in tool
      `-- MCP server tool
      |
      v
Typed result -> Agent event -> Service commit -> Timeline
      |
      `------> bounded next model input
```

Skills supply instructions and available capabilities; they are not a third
tool execution backend. This diagram describes the current Kernel integration;
future native Providers must prove their capability/policy boundaries explicitly.
Policy, sandbox, MCP initialization, and schema failures remain visible. The
Provider must not retry through a weaker execution path without an explicit
policy decision.

## MCP implementation rules

- Keep tool and call-state mutation in existing MCP connection/tool call manager boundaries. Do not thread mutation through unrelated function parameters.
- Reuse connections according to incremental checks, lifecycle state, and negotiated capabilities; avoid unnecessary session or connection resets.
- Strongly type servers, tools, resources, prompts, call results, capabilities, and transport configuration. Model user configuration, runtime state, source, installation/startup method, and errors separately.
- Persist auditable startup commands, environment variables, working directories, and permission boundaries for external servers. Surface initialization failures, schema mismatches, disconnections, and permission errors explicitly.

## Context rules

These constraints govern context injected by Talos runtime code, not the development assistant's file reads, documentation, or work reports.

- Build context incrementally without rewriting historical items. Preserve content order and structure to avoid unnecessary cache invalidation.
- Represent injected fragments as typed context objects using the project's injection interface; record source, boundaries, trimming policy, and token-cost assumptions instead of scattering string concatenation.
- Bound every injected item, list, log, file fragment, and tool result before model injection. Enforce the configured hard cap; no individual item may exceed 10K tokens.
- Mark new items that may exceed 1K tokens as high risk and obtain human review before merge or release. Prepare the implementation, cap/trimming verification, and review materials first; do not mark delivery complete before review.
- Explicit compaction changes Provider context and exposes its operation through canonical events; it does not rewrite committed product history.

## State rules

- Configuration owns defaults, root paths, and derived merging, filtering, and deduplication. Pass configuration/context objects instead of scattered scalars. Tests may override config but must not alter real defaults to fit fixtures.
- Database-backed capabilities belong in an owning `xxx-store` module or crate with `XxxRecord` and `XxxStore`; keep unrelated business stores separate. Records stay within the store/repository boundary. Services convert them to business DTOs or `XxxInfo` / `XxxResponse` for SDK, Gateway, protocol, and UI callers.
- Design persistent writes for concurrency, crashes, and partial failure. Use locks, temporary writes, atomic replacement, and fsync where required. Multi-resource updates, including directory-plus-state changes, define ordering, rollback, and recovery.
- Never delete old data before its replacement is durable or permanently remove an installation before recording its state. Crash cleanup consults state records, not names alone.
- Terminal Turn states are written only after the corresponding outcome is known.

## Implementation and upgrade references

- [Admission driver](../../talos-rs/crates/orchestration/service/src/turn/agent_execution.rs)
- [Commit writer](../../talos-rs/crates/orchestration/service/src/turn/agent_execution/timeline_writer.rs)
- [Execution Store](../../talos-rs/crates/orchestration/store/src/session/agent_execution.rs)
- [Protocol and data upgrade boundaries](ERROR_MODEL.md#upgrade-and-rollback):
  coordinated protocol 0.4 consumers, transactional migrations and backup restore.
  Archived legacy Timeline data is not automatically exposed as current History.
