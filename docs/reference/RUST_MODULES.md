# Rust workspace modules

[Chinese](RUST_MODULES_zh.md)

The Talos Rust workspace currently contains 46 members. `talos-rs/Cargo.toml`
is the authoritative membership source; this document explains ownership rather
than replacing the manifest.

| Area | Count | Packages |
| --- | ---: | --- |
| Common | 10 | `talos-llm-provider`, provider implementations, `talos-fs`, `talos-prompt`, `talos-types` |
| Infrastructure | 6 | `talos-error`, `talos-config`, `talos-logging`, `talos-storage`, `talos-auth`, `talos-automation` |
| Perception | 1 | `talos-channel` |
| Cognition | 1 | `talos-memory` |
| Action | 10 | `talos-mcp`, `talos-plugin`, `talos-skills`, tools, policy, sandbox SPI, and platform sandboxes |
| Orchestration | 6 | `talos-agent-kernel`, `talos-agent`, `talos-store`, `talos-service`, `talos-router`, `talos-coordinator` |
| Gateway | 4 | `talos-gateway`, `talos-gateway-http`, `talos-app-protocol`, `talos-app-server` |
| Application composition | 2 | `talos-base`, `talos-upgrade` |
| Apps and SDKs | 4 | `talos-sdk`, `talos-sdk-embedded`, `talos-app-server-test-client`, `talos-tui` |
| Binaries | 2 | `app-server`, `talos-server` |

## Common

- `crates/common/llm/`: provider contract plus Anthropic, Bailian, DeepSeek,
  Ollama, OpenAI, and OpenRouter implementations.
- `crates/common/fs/`: bounded and safe filesystem operations.
- `crates/common/prompt/`: prompt and dynamic context construction.
- `crates/common/types/`: shared domain and API objects.

## Infrastructure and capabilities

- `crates/infra/`: errors, configuration, logging, storage, authentication, and automation.
- `crates/perception/channels/`: external channel adapters.
- `crates/cognition/memory/`: memory persistence and retrieval behavior.
- `crates/action/`: tools, Skills, plugins, MCP, permission policy, and sandbox execution.

## Orchestration and gateways

- `crates/orchestration/store/`: database records and store interfaces.
- `crates/orchestration/service/`: product-facing application service facade.
- `crates/orchestration/router/` and `coordinator/`: Agent binding and dispatch.
- `crates/orchestration/agent/` and `agent-kernel/`: runtime contract and model-driven execution.
- `crates/gateway/`: shared Gateway API, HTTP, JSON-RPC app-server, and protocol definitions.

## Composition and entry points

- `crates/app/talos-base/`: shared runtime composition for product binaries and embedded clients.
- `crates/app/talos-upgrade/`: release upgrade capability.
- `apps/sdk/` and `apps/sdk-embedded/`: protocol and in-process Rust SDKs.
- `apps/tui/`: the `talos` terminal product.
- `bin/app-server/`: local JSON-RPC adapter binary.
- `bin/talos-server/`: HTTP and Web UI server binary.

When membership changes, update this document and its Chinese counterpart in
the same change after updating `Cargo.toml`.
