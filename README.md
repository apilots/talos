# Talos

[Chinese](README_zh.md)

Talos is an AI agent platform with durable sessions, provider-based execution,
shared client contracts, and governed tool access across terminal, web, desktop,
HTTP, and embedded clients. The Rust workspace owns the agent runtime,
session and turn lifecycle, model integration, memory, tools, Skills, MCP, and
sandboxing. The TypeScript workspace provides the Web UI, Desktop shell, and
client SDKs.

## Product entry points

| Entry point | Kind | Purpose |
| --- | --- | --- |
| `talos` | Product | TUI and non-interactive terminal commands using the embedded runtime |
| `talos-server` | Product | HTTP API, SSE timeline streaming, and hosted Web UI server |
| Web UI | Product surface | React UI served in development or by `talos-server` |
| Desktop | Product | Electron shell backed by the local app-server |
| `app-server` | Internal adapter | JSON-RPC over stdio for Desktop and protocol tests |

## Core capabilities

- Session, turn, project, and timeline lifecycle management
- Model-neutral agent execution with multiple LLM providers
- Persistent memory and bounded context construction
- Durable execution events and independent product notices with replayable history
- Structured errors shared by requests, streams, history, and client presenters
- Built-in tools, Skills, MCP server plugins, policy, and sandbox execution
- Parent-child subagent collaboration through the same runtime dispatch path
- SDK-first access and shared domain semantics across Web, Desktop, TUI, and gateways
- Capability parity as the required contract for TypeScript, Rust, and embedded SDK evolution

## Architecture direction

Talos is both an Agent platform for its first-party products and a framework for
higher-level applications. It evolves through stable, language-neutral product
contracts, aligned SDKs, layered runtime ownership, and composable capability
domains.

```text
+----------------------------------------------------------------------------+
| Talos products and higher-level applications                               |
+----------------------------------------------------------------------------+
| Talos framework                                                            |
| Language-neutral contracts | Client SDKs | Embedded SDK | Gateways         |
+------------------------+------------------------+--------------------------+
| Perception             | Cognition              | Action                   |
| Inputs | Channels      | Memory | Reasoning     | Tools | Skills | MCP     |
+------------------------+------------------------+--------------------------+
| Product lifecycle and durable execution                                    |
| Session / Turn | Agent binding | Admission -> Commit -> Replay             |
| Harness -> AgentProvider -> AgentSession | Subagent collaboration          |
| Kernel Provider [current] | Additional Agent Providers [planned]           |
+----------------------------------------------------------------------------+
| Shared product facts                                                       |
| Agent events + SystemNotice -> one durable Timeline -> History / clients   |
+----------------------------------------------------------------------------+
| Infrastructure                                                             |
| Store | Storage | Config | Auth | Automation | Logging | Observability     |
+----------------------------------------------------------------------------+
| Metacognition [planned cross-cutting evolution]                            |
| Evaluation | Self-monitoring | Learning | Identity and personality         |
+----------------------------------------------------------------------------+
```

This is the long-term layering direction, not an execution sequence. The same
layers are expanded with current and planned status in
[System architecture](docs/architecture/ARCHITECTURE.md).

Multiple LLM providers are supported by the current Kernel Agent. Codex, Claude
Code and Pi Agent Providers remain planned, not selectable implementations.
Perception/Cognition/Action describe capabilities rather than a second pipeline.

## Operating boundaries

- A Session binds Agent, User and Workspace; shared memory follows User + Workspace.
- Accepted input is not completed work. Waiting can resume the same logical Turn;
  refresh reconnects/replays instead of re-submitting the original input.
- Recovery requires a supported checkpoint/continuation. Arbitrary-position
  resume and exactly-once external tool effects are not guaranteed.
- Computer owners authorize execution and environment configuration. Direct/outbound
  transport is independent of ownership. See [Deployment](deploy/README.md).
- `TurnCompleted` does not mean task acceptance passed. Generic completion gates
  and evidence-driven learning remain [evolution topics](docs/architecture/ARCHITECTURE.md#evolution-model).

## Repository map

```text
talos/
|-- talos-rs/       Rust runtime, gateways, SDKs, TUI, and server binaries
|-- talos-app/      Web, Desktop, and TypeScript SDK workspace
|-- talos-e2e/      Cross-component integration and release scenarios
|-- deploy/         Server deployment and TUI installation assets
|-- conf/           Repository-managed configuration and migrations
|-- docs/           Current architecture, reference, and development docs
|-- spec/           Active and archived change specifications
|-- build/          Product packaging and branding resources
`-- scripts/        Repository automation
```

## Quick start

Prerequisites are Rust and Cargo, Node.js 24, pnpm 9.14.4, and `just`. Bootstrap
the repository tools and frontend dependencies:

```bash
cargo install --locked just
just install-tools
just setup-git-hooks
pnpm --dir talos-app install
```

The default model uses DeepSeek. Export its key, then run the Web UI and server
in separate terminals:

```bash
export TALOS_LLM_DEEPSEEK_API_KEY="your-api-key"
just dev-server
just dev-web
```

Run the terminal client:

```bash
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos
```

## Common development commands

```bash
just fmt-check
just check -p talos-service
just test -p talos-service
just e2e-case 01_health_and_models
```

Prefer package-scoped checks during development. See
[Development](docs/development/DEVELOPMENT.md) for the full workflow.

## Documentation

- [Documentation index](docs/README.md)
- [System architecture](docs/architecture/ARCHITECTURE.md)
- [Domain model and scope](docs/architecture/DOMAIN_MODEL.md)
- [Runtime architecture](docs/architecture/RUNTIME_ARCHITECTURE.md)
- [Memory architecture](docs/architecture/MEMORY_ARCHITECTURE.md)
- [Client architecture](docs/architecture/CLIENT_ARCHITECTURE.md)
- [Error model](docs/architecture/ERROR_MODEL.md)
- [Project structure](docs/reference/PROJECT_STRUCTURE.md)
- [Rust workspace modules](docs/reference/RUST_MODULES.md)
- [Rust workspace](talos-rs/README.md)
- [Web, Desktop, and TypeScript SDKs](talos-app/README.md)
- [End-to-end tests](talos-e2e/README.md)
- [Deployment](deploy/README.md)

Change-specific designs live under [`spec/`](spec/). The
[`spec/REGISTRY.md`](spec/REGISTRY.md) file is the authoritative index for
active and archived specifications.

## License

Talos is licensed under the [Apache License, Version 2.0](LICENSE). Talos
attributions are in [NOTICE](NOTICE), and dependency notices are in
[THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES).
