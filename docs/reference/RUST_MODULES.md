# Rust workspace modules

[Chinese](RUST_MODULES_zh.md)

The Talos Rust workspace currently contains 55 members. `talos-rs/Cargo.toml`
is the authoritative membership source; this document explains ownership rather
than replacing the manifest.

| Area | Count | Packages |
| --- | ---: | --- |
| Common | 11 | `talos-llm-provider`, provider implementations, `talos-fs`, `talos-knowledge-format`, `talos-prompt`, `talos-types` |
| Infrastructure | 7 | `talos-error`, `talos-config`, `talos-logging`, `talos-storage`, `talos-auth`, `talos-automation`, `talos-system-info` |
| Perception | 1 | `talos-channel` |
| Cognition | 1 | `talos-memory` |
| Action | 10 | `talos-mcp`, `talos-plugin`, `talos-skills`, tools, policy, sandbox SPI, and platform sandboxes |
| Orchestration | 12 | `agent-pb`, `agent-protocol`, `agent-client`, `talos-agent-api`, `talos-run-control`, `talos-agent-kernel`, `talos-agent-provider-kernel`, `talos-agent`, `talos-store`, `talos-service`, `talos-router`, `talos-coordinator` |
| Gateway | 4 | `talos-gateway`, `talos-gateway-http`, `talos-app-protocol`, `talos-app-server` |
| Application composition | 3 | `talos-base`, `talos-worker`, `talos-upgrade` |
| Apps and SDKs | 4 | `talos-sdk`, `talos-sdk-embedded`, `talos-app-server-test-client`, `talos-tui` |
| Binaries | 3 | `app-server`, `talos-server`, `talos-worker` |

## Common

- `crates/common/llm/`: provider contract plus Anthropic, Bailian, DeepSeek,
  Ollama, OpenAI, and OpenRouter implementations.
- `crates/common/fs/`: bounded and safe filesystem operations.
- `crates/common/knowledge-format/`: shared structured knowledge document format.
- `crates/common/prompt/`: prompt and dynamic context construction.
- `crates/common/types/`: shared domain and API objects.

## Infrastructure and capabilities

- `crates/infra/`: errors, configuration, logging, storage, authentication, automation, and system metadata.
- `crates/infra/system-info/`: bounded Linux/macOS hardware collection for Worker registration; exposes `collect()` using sysinfo and wgpu. Identity, authorization, and heartbeat remain with their existing owners.
- `crates/perception/channels/`: external channel adapters.
- `crates/cognition/memory/`: typed memory documents and reader/writer ports;
  Service owns product policy and Store owns Dream Memory persistence/indexing.
- `crates/action/`: tools, Skills, plugins, MCP, permission policy, and sandbox execution.

## Orchestration and gateways

- `crates/orchestration/store/`: database records and store interfaces.
- `crates/orchestration/service/`: product-facing application service facade.
- `crates/orchestration/router/` and `coordinator/`: Agent binding and dispatch.
- `crates/orchestration/agent-api/`: public Agent messages, events and history contracts.
- `crates/orchestration/agent-pb/`, `agent-protocol/`, and `agent-client/`: generated gRPC API,
  shared Harness wire projection, and transport clients.
- `crates/orchestration/agent/`: Provider SPI, Harness lifecycle and event consistency.
- `crates/orchestration/run-control/`: cancellation and task cleanup primitives.
- `crates/orchestration/agent-provider-kernel/`: the SPI-to-Kernel adaptation boundary.
- `crates/orchestration/agent-kernel/`: private model-driven execution engine.
- `crates/gateway/`: shared Gateway API, HTTP, JSON-RPC app-server, and protocol definitions.

## Composition and entry points

- `crates/app/talos-base/`: shared runtime composition for product binaries and embedded clients.
- `crates/app/talos-worker/`: shared Agent execution composition for embedded and standalone hosts.
- `crates/app/talos-upgrade/`: release upgrade capability.
- `apps/sdk/` and `apps/sdk-embedded/`: protocol and in-process Rust SDKs.
- `apps/tui/`: the `talos` terminal product.
- `bin/app-server/`: local JSON-RPC adapter binary.
- `bin/talos-server/`: HTTP and Web UI server binary.
- `bin/talos-worker/`: standalone Agent worker binary.

When membership changes, update this inventory from `Cargo.toml`. Historical
translations must link to the current inventory rather than claim a stale count.
