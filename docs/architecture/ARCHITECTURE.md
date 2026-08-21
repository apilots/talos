# Talos system architecture

[Chinese](ARCHITECTURE_zh.md)

## Purpose

This document defines the stable system boundaries of Talos. It describes the
current architecture and the extension points that should carry future work. It
is not a crate inventory or an implementation roadmap.

## Overall architecture

This ownership map expands the same long-term layers shown in the repository
README. It is not an execution sequence. `[current]` identifies implemented
boundaries; `[planned]` identifies an explicit evolution target.

```text
+--------------------------------------------------------------------------------+
| Products and integrations [current]                                            |
| Web | Desktop | TUI | Command mode | External applications | Channels          |
+-----------------------------------------+--------------------------------------+
                                          |
                                          v
+--------------------------------------------------------------------------------+
| Client and embedded SDK capability surface                                     |
| TypeScript: Web -> Browser TalosClient | Desktop -> AppServerClient [current]  |
| Rust: TUI/command -> EmbeddedServerClient | AppServerClient [current]          |
| Language-neutral domain contract [current] | Cross-SDK parity [planned]        |
+-----------------------------------------+--------------------------------------+
                                          |
                                          v
+--------------------------------------------------------------------------------+
| Gateway and application lifecycle [current]                                    |
| Transport: Browser HTTP/SSE | Desktop JSON-RPC stdio | TUI Embedded Gateway    |
| Channels -> adapters | auth | validation | protocol and event projection       |
| TalosService: Session | Turn | Project | Timeline | Plugin | user interaction  |
+-----------------------------------------+--------------------------------------+
                                          |
                                          v
+--------------------------------------------------------------------------------+
| Agent orchestration and runtime                                                |
| Router / Coordinator / AgentRegistry / multiple Harnesses [current]            |
| Selection -> AgentHarness -> AgentRuntime SPI [current, Kernel-specialized]    |
|   +-- KernelAgent [current local default] -> Agent Kernel reasoning/tool loop  |
|   +-- CloudAgent [planned cloud default] -> remote cloud runtime               |
|   +-- Codex runtime [planned]                                                  |
|   `-- Claude runtime [planned]                                                 |
| AgentEvent / AgentRunResult / capabilities / interrupt / resume [current]      |
+--------------------------+--------------------------+--------------------------+
| Semantic capability domains; these columns are not an execution sequence       |
+--------------------------+--------------------------+--------------------------+
| Perception               | Cognition                | Action                   |
| Channel I/O [current]    | Memory/Prompt [current]  | Tools/Skills [current]   |
| Media [current]          | Context/LLM [current]    | MCP/Policy [current]     |
| Environment [planned]    | Strategy [planned]       | Approval [current]       |
|                          |                          | Sandbox [current]        |
+--------------------------+--------------------------+--------------------------+
| Infrastructure [current]                                                       |
| Store | Storage | Filesystem | Config | Auth | Automation | Logging            |
+--------------------------------------------------------------------------------+

+--------------------------------------------------------------------------------+
| Metacognition [planned cross-cutting evolution]                                |
| Evaluation | Self-monitoring | Learning | Identity and personality             |
| Observes defined layers and changes state only through their typed contracts.  |
+--------------------------------------------------------------------------------+
```

The capability-domain rows describe semantic ownership, not a second request
sequence or a one-to-one crate map. Channel integrations enter through their
transport and Gateway adapters; Perception owns event/media normalization and
outbound adaptation. The selected runtime consumes Cognition and Action
capabilities while retaining ownership of its reasoning and tool loop.

## Boundary ownership

| Boundary | Owns | Must not own |
| --- | --- | --- |
| Products and integrations | First-party interaction plus higher-level application composition | Runtime business rules |
| Client SDKs | Language-neutral product semantics, complete capability API, shared domain objects, and transport selection | Server records or UI state |
| Gateways | Transport, authentication, validation, protocol projection | Agent execution policy |
| Application service | Product lifecycle and domain orchestration | Protocol envelopes or rendering |
| Coordinator and harness | Agent binding, dispatch, and runtime lifecycle | Client-specific behavior |
| Agent definition | Stable Agent identity, runtime family, model, and typed runtime config | Runtime-private handles |
| Agent runtime SPI | Common run, stream, resume, capability, and event contract | Routing or product lifecycle |
| Agent kernel | Built-in KernelAgent reasoning and tool-loop subsystem | Other runtime families or durable product lifecycle |
| Perception | External event and media normalization plus inbound/outbound Channel adaptation | Session lifecycle or Agent decisions |
| Cognition | Memory, bounded context, model and prompt capabilities | Product lifecycle or side-effect policy |
| Action | Tool, Skill, MCP, permission, approval, and sandbox capabilities | Agent routing or protocol projection |
| Infrastructure | Technical persistence, configuration, and observability | Product policy |
| Metacognition (planned) | Evaluation, self-monitoring, learning, identity, and personality through explicit contracts | Hidden state mutation or bypassing lifecycle and policy |

## Stable domain flow

All clients enter the same product lifecycle:

```text
Client request
     |
     v
Gateway validation and projection
     |
     v
Application service creates or resumes Session and Turn
     |
     v
Coordinator resolves the Session's Agent binding
     |
     v
Agent harness invokes the selected concrete runtime through AgentRuntime SPI
     |
     v
Concrete runtime emits standardized events and results
     |
     v
SDK projects the same domain result through its selected transport
```

HTTP, JSON-RPC, and embedded clients may use different envelopes, but they must
not define different Session, Turn, Timeline, Tool, or Agent semantics.

## Core domain terms

| Term | Meaning |
| --- | --- |
| `Session` | Durable conversation and Agent-binding boundary containing multiple Turns |
| `Turn` | One durable execution initiated inside a Session |
| `Timeline` | Session-addressed ordered event stream; events identify their owning Turn |
| `AgentDefinition` | Configured Agent identity selecting one runtime family and its typed config |
| `AgentHarness` | Common lifecycle facade registered under one Agent id |
| `AgentRuntime` | Internal SPI implemented by concrete runtime backends |
| Agent Kernel | Built-in KernelAgent subsystem (`talos-agent-kernel`) for one bounded Turn reasoning and tool loop |

## Dependency direction

```text
Product binaries and user surfaces
                 |
                 v
SDK product API
                 |
                 v
Transport and Gateway adapters
                 |
                 v
Application service and orchestration
                 |
                 v
Agent runtime and focused capabilities
                 |
                 v
Infrastructure and shared types
```

Dependencies point toward stable abstractions. Lower layers do not depend on
upper product composition. Database records remain inside store boundaries;
protocol envelopes remain inside gateway boundaries; rendering state remains
inside clients.

Rust is the current canonical runtime implementation. Language-neutral product
contracts and generated/shared domain types enable aligned SDKs without
requiring independent per-language copies of the runtime and domain model.

## Cross-cutting invariants

- `Session` and `Turn` are the durable lifecycle boundary for agent work.
- `Timeline` is the ordered source for user-visible runtime progress.
- An Agent binding is resolved by the Coordinator, not selected inside a tool or client.
- Multiple Agent definitions may be registered; each enabled, supported definition resolves to one Harness and concrete runtime.
- Runtime implementations emit the same Agent events and results through the AgentRuntime SPI.
- Local composition defaults to KernelAgent; cloud composition will default to CloudAgent.
- Subagents create child Sessions and Turns and re-enter the normal dispatch path.
- Perception, Cognition, and Action remain focused capability domains with explicit owners.
- Metacognition may observe and evaluate the system, but cannot bypass Service, Runtime, policy, or state boundaries.
- Core domain objects are strongly typed and shared across protocol projections.
- Context, tool results, file reads, and list APIs have explicit size bounds.
- State writes account for concurrency, partial failure, and crash recovery.
- Tool permission and sandbox decisions fail explicitly; no silent fallback weakens policy.

## Evolution model

Future capabilities should extend an existing boundary:

| Change | Extension boundary |
| --- | --- |
| New model provider | LLM provider implementation plus config, composition, and provider tests |
| New Agent runtime family | Generalize the current Kernel-specialized Harness/input bridge, then add typed config, an AgentRuntime implementation, a factory branch, and capability tests |
| New tool or external service | Tool registry, Skill, or MCP capability |
| New execution isolation | Sandbox SPI and platform implementation |
| New client or protocol | Extend the SDK first; place protocol differences in its transport or Gateway adapter |
| New language SDK | Reuse the language-neutral product contract and prove capability parity |
| New persistence backend | Store or storage implementation behind its interface |
| New channel | Perception/channel adapter into the same application service |
| New metacognition capability | Add an explicit observer/evaluator boundary; state changes re-enter existing typed Service APIs |

Create a new crate only when no existing owner can preserve cohesion. Do not add
parallel managers, registries, event buses, or protocol-specific domain models
for speculative future use.

## Detailed documents

- [Runtime architecture](RUNTIME_ARCHITECTURE.md)
- [Client architecture](CLIENT_ARCHITECTURE.md)
- [Error model](ERROR_MODEL.md)
- [Rust workspace modules](../reference/RUST_MODULES.md)
