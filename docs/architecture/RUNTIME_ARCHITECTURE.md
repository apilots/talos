# Agent Provider architecture

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
Standard AgentEvent / AgentRunResult -> Timeline
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
Router / Coordinator selects one registration for a Session or Turn
```

Multiple Agent definitions may use the same Provider implementation with
different identities, models, workspaces, or typed Provider configuration. A
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
ordered model rather than reconstructing execution state from transport details.

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
Provider must not retry through a weaker execution path without an explicit
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
