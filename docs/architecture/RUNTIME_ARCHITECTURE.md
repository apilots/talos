# Runtime architecture

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
    +-- applies the common runtime lifecycle and event contract
    |
    v
AgentRuntime SPI
    |
    +-- KernelAgent [current local default] -> Agent Kernel subsystem
    +-- CloudAgent [planned cloud default] -> remote cloud runtime
    +-- CodexAgent [extension] -> runtime-private backend
    `-- ClaudeAgent [extension] -> runtime-private backend
    |
    v
Standard AgentEvent / AgentRunResult -> Timeline
```

`TalosService` is the product-facing facade. Protocol adapters call it instead
of constructing stores, runtimes, or registries directly. The Agent Kernel
subsystem (`talos-agent-kernel`) is the built-in `KernelAgent` engine; it is not
the universal implementation behind every future Agent runtime.

## Multiple Agents and runtime SPI

Multi-Agent registration and runtime implementation are separate dimensions:

```text
Agent configuration
    |
    +-- AgentDefinition: default  -> kind=kernel -> KernelAgent
    +-- AgentDefinition: reviewer -> kind=kernel -> KernelAgent
    +-- AgentDefinition: cloud    -> kind=cloud  -> CloudAgent [planned]
    +-- AgentDefinition: coder    -> kind=codex  -> CodexAgent [extension]
    `-- AgentDefinition: analyst  -> kind=claude -> ClaudeAgent [extension]
    |
    v
AgentDefinitionLoader
    |
    v
AgentHarnessFactory selects the runtime family from typed config
    |
    v
AgentRegistry stores multiple AgentHarness registrations by Agent id
    |
    v
Router / Coordinator selects one registration for a Session or Turn
```

Multiple Agent definitions may use the same runtime family with different
identities, models, workspaces, or typed runtime configuration. A new runtime
family targets the internal `AgentRuntime` SPI and remains behind the common
Harness lifecycle; Coordinator must not downcast to a concrete runtime.

This is the target extension boundary, not yet a fully generic plug-in point.
The current Harness stores `KernelAgent` directly, its parameters include
Kernel-specific config, and the SPI input-delivery type comes from
`talos-agent-kernel`. CloudAgent and other runtime families therefore require
generalizing that container and input bridge before adding their implementations.

Current implementation status is explicit: local composition defaults to the
implemented `KernelAgent`. Cloud composition will default to `CloudAgent`, which
requires a new typed kind, config, runtime implementation, and factory branch.
`AgentKind` already recognizes `Codex` and `Claude`, but their typed runtime
configs and runtime implementations are not implemented, so the Harness factory
fails instead of falling back to Kernel.

## Kernel-backed Turn execution

The following flow is the built-in `KernelAgent` path. Other runtime families
may use different private backends, but must return the same standardized Agent
events, results, capabilities, interrupt behavior, and resume behavior.

```text
Turn request
     |
     v
Validate request and Session state
     |
     v
Persist user input and create the Turn
     |
     v
Resolve Agent binding through Coordinator
     |
     v
Build bounded context from history, memory, Skills, and project inputs
     |
     v
Call the selected model provider
     |
     +-- text/reasoning delta ----------> Timeline
     |
     +-- tool request -> policy -> approval -> sandbox/tool execution
     |                                      |
     |                                      v
     |                              tool result -> Timeline
     |                                      |
     +--------------------------------------+
     |
     v
Persist final response and terminal Turn state
```

The Timeline preserves ordering across model output, tool activity, user
interaction, errors, and the final response. Clients render or project that
ordered model rather than reconstructing runtime state from transport details.

## Subagent execution

Subagent collaboration is orthogonal to Agent runtime registration.

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
Re-enter Coordinator -> same AgentHarness -> same AgentRuntime implementation
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
      +-- denied -------------------------> explicit error event
      |
      v
Execution plan and sandbox selection
      |
      +-- built-in tool
      +-- Skill-backed instruction flow
      `-- MCP server tool
      |
      v
Typed result -> bounded Timeline event -> next model input
```

Policy, sandbox, MCP initialization, and schema failures remain visible. The
runtime must not retry through a weaker execution path without an explicit
policy decision.

## Context rules

- Context is built incrementally; historical items are not rewritten.
- Each injected item has a source, boundary, truncation rule, and token budget.
- Single items cannot exceed the configured hard cap.
- Tool results and file content are bounded before model injection.
- Compaction produces explicit durable history rather than an invisible client-only summary.

## State rules

- Store records remain internal to the store boundary.
- Service APIs expose domain request and response objects, not database records.
- File state uses locking, temporary writes, and atomic replacement where required.
- Multi-resource updates define ordering, rollback, and recovery behavior.
- Terminal Turn states are written only after the corresponding outcome is known.
