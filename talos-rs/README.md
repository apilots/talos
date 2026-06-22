# Talos AI Agent Framework

Talos is an AI Agent Framework designed to serve as the software brain for AI assistants similar to Jarvis. This framework includes comprehensive functionality for running AI agent services with multi-channel support and advanced capabilities.

## ✨ Features Implemented

### Infrastructure Layer
- **Error Handling**: Centralized error management
- **Logging**: Structured logging with configurable levels
- **Configuration**: YAML-based configuration with environment variable support
- **Storage**: SQLite-based storage with automatic schema setup
- **Authentication**: JWT-based authentication system

### Cognitive Layer
- **Memory System**: Short-term and long-term memory with unified interface
- **Prompt Builder**: Shared prompt composition with configurable planner modes
- **LLM Providers**: Support for Anthropic, OpenAI, Bailian, and Ollama-compatible endpoints

### Action Layer
- **Tool System**: Built-in tools plus deferred discovery through `tool_search`
- **Plugin Lifecycle**: Unified install, enable, disable, update, and uninstall flow for Skills and MCP server plugins
- **Skills**: Prompt-injected instruction bundles managed through the plugin system
- **MCP Runtime**: `rmcp`-based MCP client runtime exposed to the model as on-demand external tools
- **Tool Policy**: Allow/deny list policies for tool access control
- **Sandbox Execution**: Isolated execution environments for security

### Orchestration Layer
- **Agent Core**: Main agent functionality with LLM integration
- **Session Management**: Database-backed session storage with multi-channel support
- **Routing System**: Message routing to appropriate agents
- **Coordination**: Multi-agent coordination and management

### Gateway Layer
- **HTTP Server**: Session-first REST API (`/v1/sessions` + `/v1/turns/start`)
- **App Server**: Local JSON-RPC over stdio JSONL for CLI, TUI, and Desktop
- **Application Service**: Shared service layer used by HTTP, app-server, SDK, and WebUI adapters
- **Plugin API**: Shared API surface for plugin source, lifecycle, search, read, and MCP/Skill detail flows
- **Authentication Middleware**: Bearer token validation

### Perception Layer
- **Channel Integration**: Unified interface for IM platforms (Telegram, Discord, etc.)
- **TUI (Terminal UI)**: Rich terminal-based interface with keyboard navigation

### Unified Client Interface
- **Standardized API**: All clients (TUI, CLI, mobile apps, third-party) use same interface
- **Gateway Client**: Centralized communication layer for consistent client-server interaction

## 🚀 Getting Started

### Prerequisites
- Rust 1.70+ (installation script provided)

### Installation

1. Clone the repository
```bash
git clone <repository-url>
cd talos/talos-rs
```

2. Install Rust and build the project
```bash
./build.sh
```

### Configuration

1. Choose the runtime home if you do not want the default `${HOME}/.talos`:
```bash
mkdir -p "${TALOS_HOME:-$HOME/.talos}/conf"
```

2. Start Talos. It creates `${TALOS_HOME:-$HOME/.talos}/conf/talos.yaml` and `${TALOS_HOME:-$HOME/.talos}/.env` on first run.

3. Put provider secrets in `${TALOS_HOME:-$HOME/.talos}/.env`:
```bash
TALOS_LLM_ANTHROPIC_API_KEY="your-anthropic-api-key"
# or
TALOS_LLM_OPENAI_API_KEY="your-openai-api-key"
TALOS_TELEGRAM_BOT_TOKEN="your-telegram-bot-token"  # for Telegram integration
```

### Running the Server

### Entry Points

| Binary | Package | Purpose |
|--------|---------|---------|
| `talos` | `talos-ui-tui` | TUI user entrypoint |
| `app-server` | `app-server` | Private JSON-RPC stdio runtime used by TUI, Desktop, and SDK clients |
| `talos-server` | `talos-server` | HTTP API and WebUI server |
| `talos-cli` | `talos-cli` | CLI binary for App Server flows |

`app-server` is not the product server. It is a local runtime process and does
not listen on a port. `talos-server` is the deployable HTTP/WebUI server.

Start the Talos server:
```bash
cargo run -p talos-server --bin talos-server
```

The server will start on `http://0.0.0.0:8080` by default.

Start the local app-server protocol adapter:
```bash
cargo run -p app-server --bin app-server
```

`app-server` speaks JSON-RPC over stdin/stdout JSONL. It does not open a
port; stdout is reserved for protocol messages and logs go to stderr.

Run CLI through the embedded app-server path:
```bash
cargo run -p talos-cli --bin talos-cli -- models
cargo run -p talos-cli --bin talos-cli -- chat simple "hello" --stream
```

### Running the TUI

Start the Terminal User Interface:
```bash
cargo run -p talos-ui-tui --bin talos
```

The TUI starts an embedded `app-server` child process and communicates
over JSON-RPC stdio without opening a local port.

### API Usage

Start a chat with a client supplied session id:
```bash
SESSION_ID="sess_$(uuidgen | tr '[:upper:]' '[:lower:]' | tr -d '-')"

curl -X POST "http://localhost:8080/v1/turns/start" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-token-here" \
  -d '{
    "session_id": "'"${SESSION_ID}"'",
    "input": [
      { "type": "text", "text": "Hello, how are you?" }
    ],
    "workspace_id": null,
    "context": {
      "source_type": "http",
      "provider": "http",
      "channel_id": "default",
      "user_id": "user123",
      "thread_id": null,
      "message_id": "msg-start-1",
      "trace_id": "trace-start-1"
    },
    "model": "gpt-4o-mini",
    "project_id": null,
    "current_directory": null,
    "client_request_id": null
  }'
```

Send another message inside an existing session:
```bash
curl -X POST http://localhost:8080/v1/turns/start \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-token-here" \
  -d '{
    "session_id": "<session_id>",
    "input": [
      { "type": "text", "text": "Hello, how are you?" }
    ],
    "workspace_id": null,
    "context": null,
    "model": null,
    "project_id": null,
    "current_directory": null,
    "client_request_id": null
  }'
```

Health check:
```bash
curl http://localhost:8080/health
```

## 🏗️ Architecture Overview

Talos follows an eight-layer architecture with clear separation of concerns:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│  Clients (TUI, CLI, Mobile Apps, Third-party)                             │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  SDK Client - Unified API Interface (talos-sdk)                           │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Gateway Layer - HTTP API, Authentication, Rate Limiting                  │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Orchestration Layer - Agent Coordination, Session Management, Routing     │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Cognition Layer - Memory, Prompting, Model Selection                      │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Action Layer - Tool Execution, Policy, Sandbox                            │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Perception Layer - Channels And TUI                                       │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Infrastructure Layer - Storage, Config, Logging, Error Handling           │
└─────────────────────────────────────────────────────────────────────────────┘
```

## 📁 Project Structure

- `crates/common/` - Shared utilities and LLM abstractions
- `crates/common/fs/` - Shared filesystem helper for atomic writes, directory operations, and file locks
- `crates/infra/` - Infrastructure components (config, logging, storage, auth)
- `crates/perception/` - Channel integrations
- `crates/cognition/` - Memory-related services
- `crates/action/` - Tools, MCP runtime, Skills, Plugin lifecycle, policy, and sandbox execution
- `crates/orchestration/` - Agent coordination, kernel, sessions, routing, and application services
- `crates/gateway/` - HTTP gateway, app-server JSON-RPC adapter, and protocol types
- `crates/app/talos-base/` - Shared runtime base used by `app-server` and `talos-server`
- `crates/orchestration/application-service/` - Shared application services used by protocol adapters
- `apps/sdk/` - Rust SDK package (`talos-sdk`)
- `bin/app-server/` - App Server binary
- `bin/talos-server/` - HTTP/WebUI Server binary
- `apps/ui-tui/` - TUI package that builds the `talos` binary

## ⚙️ Configuration Options

The system can be configured via `${TALOS_HOME:-$HOME/.talos}/conf/talos.yaml` and `${TALOS_HOME:-$HOME/.talos}/.env`:

- `TALOS_LLM_ANTHROPIC_API_KEY` - API key for Anthropic services
- `TALOS_LLM_OPENAI_API_KEY` - API key for OpenAI services
- `TALOS_LLM_BAILIAN_API_KEY` - API key for Bailian services
- `GATEWAY_TOKEN` - Authentication token for the API
- `TALOS_TELEGRAM_BOT_TOKEN` - Token for Telegram bot integration
- `TALOS_LOG_LEVEL` - Logging level (debug, info, warn, error)

## 🔒 Security Features

- JWT-based authentication for API requests
- Configurable tool policies (whitelist/blacklist)
- Sandboxed tool execution environments
- Secure session management
- Environment variable support for sensitive data
- Channel-specific access controls

## 📈 Next Steps

Future enhancements include:
- Enhanced memory management with vector storage
- Advanced reasoning and planning capabilities
- Multi-agent collaboration
- Additional integration channels (WhatsApp, Slack, Discord)
- Web-based administration interface

## 🤝 Contributing

We welcome contributions! Please see our contributing guidelines for details on how to participate in the project.

## 📄 License

MIT License - see LICENSE file for details.

## 🎯 MVP Completion

✅ **MVP Version Completed** - Talos framework now includes all core functionality needed for an AI agent platform with multi-channel support, unified client interface, and extensible architecture.
