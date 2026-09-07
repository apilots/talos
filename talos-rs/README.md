# Talos Rust workspace

[Chinese](README_zh.md)

`talos-rs` contains the Talos runtime, application service, gateways, Rust SDKs,
terminal product, and server binaries. It is the owner of shared product
semantics used by Web, Desktop, TUI, HTTP, and embedded clients.

## Layout

```text
talos-rs/
|-- crates/
|   |-- common/          Shared types, LLM providers, prompts, and filesystem
|   |-- infra/           Error, config, logging, storage, auth, and automation
|   |-- perception/      External channel adapters
|   |-- cognition/       Memory capabilities
|   |-- action/          Tools, Skills, MCP, policy, and sandboxing
|   |-- orchestration/   Store, service, routing, Agents, and coordination
|   |-- gateway/         Shared Gateway, HTTP, app-server, and protocol
|   `-- app/             Runtime composition and upgrade capability
|-- apps/                Rust SDKs, TUI, and app-server test client
`-- bin/                 app-server and talos-server binaries
```

See [Rust workspace modules](../docs/reference/RUST_MODULES.md) for the current
inventory and [System architecture](../docs/architecture/ARCHITECTURE.md)
for cross-component boundaries.

## Entry points

| Binary | Package | Purpose |
| --- | --- | --- |
| `talos` | `talos-tui` | TUI and command mode using the embedded runtime |
| `app-server` | `app-server` | Local JSON-RPC over stdio adapter for Desktop and protocol tests |
| `talos-server` | `talos-server` | HTTP API, SSE, and Web UI server |

## Run

From the repository root:

```bash
just dev-server
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- exec "hello"
```

`app-server` is a protocol adapter, not the deployable product server. The TUI
uses `talos-sdk-embedded` and does not start an app-server child process.

## Validate

```bash
just fmt
just check -p talos-service
just test -p talos-service
just clippy -p talos-service
```

Prefer the smallest package or test filter that proves the change. Protocol
shape changes also require `just gen-app-protocol`.

## Boundary rules

- Gateways map transports; `TalosService` owns product lifecycle semantics.
- Store records do not cross service, protocol, SDK, or UI boundaries.
- Multiple Agent definitions register separate Harnesses; Coordinator resolves one by Agent id.
- Each Harness delegates through the `AgentProvider` SPI to a Provider-owned `AgentSession`.
- Local composition registers `KernelAgentProvider`; additional Providers use the same SPI without changing Service or Gateway semantics.
- Rust `AppServerClient` and `EmbeddedServerClient` implement the same `GatewayClient` capability contract.
- Tools, Skills, MCP, permissions, and sandbox failures remain explicit.
- Admission precedes Provider effects; Store commit precedes Timeline publication.
- Agent facts and product notices share a cursor, but only Agent facts enter execution reducers.

## Related documentation

- [Runtime architecture](../docs/architecture/RUNTIME_ARCHITECTURE.md)
- [Domain model](../docs/architecture/DOMAIN_MODEL.md)
- [Memory architecture](../docs/architecture/MEMORY_ARCHITECTURE.md)
- [Error model](../docs/architecture/ERROR_MODEL.md)
- [Development workflow](../docs/development/DEVELOPMENT.md)
- [TUI notes](apps/tui/README.md)
- [App Server protocol](crates/gateway/app-protocol/README.md)
