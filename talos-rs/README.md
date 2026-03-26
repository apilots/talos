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
- **LLM Providers**: Support for Anthropic, OpenAI, Bailian, and Ollama providers
- **Reasoning Engine**: Task planning and logical inference capabilities
- **Knowledge Management**: RAG (Retrieval Augmented Generation) and knowledge graphs

### Action Layer
- **Tool System**: Built-in tools (read, write, edit, exec, web_search)
- **Tool Policy**: Allow/deny list policies for tool access control
- **Sandbox Execution**: Isolated execution environments for security
- **Skills Framework**: Extensible skill system for complex operations

### Orchestration Layer
- **Agent Core**: Main agent functionality with LLM integration
- **Session Management**: Database-backed session storage with multi-channel support
- **Routing System**: Message routing to appropriate agents
- **Coordination**: Multi-agent coordination and management
- **Workflow Engine**: Task decomposition and execution planning

### Gateway Layer
- **HTTP Server**: Session-first REST API (`/v1/sessions` + `/v1/sessions/{id}/messages`)
- **WebSocket Server**: Real-time communication support
- **Authentication Middleware**: Bearer token validation
- **Event System**: Publish/subscribe messaging

### Perception Layer
- **Multi-channel Interface**: Support for various communication channels
- **TUI (Terminal UI)**: Rich terminal-based interface with keyboard navigation
- **Channel Abstraction**: Unified interface for IM platforms (Telegram, Discord, etc.)

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

1. Copy the example configuration:
```bash
cp talos.yaml.example talos.yaml
```

2. Edit `talos.yaml` to match your requirements

3. Set required environment variables:
```bash
export ANTHROPIC_API_KEY="your-anthropic-api-key"
# or
export OPENAI_API_KEY="your-openai-api-key"
export TELEGRAM_BOT_TOKEN="your-telegram-bot-token"  # for Telegram integration
```

### Running the Server

Start the Talos server:
```bash
cargo run -p talos
```

The server will start on `http://0.0.0.0:8080` by default.

### Running the TUI

Start the Terminal User Interface:
```bash
cargo run -p talos-ui-tui
```

### API Usage

Create a session:
```bash
curl -X POST http://localhost:8080/v1/sessions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-token-here" \
  -d '{
    "context": {
      "source_type": "http",
      "provider": "http",
      "user_id": "user123",
      "message_id": "msg-create-1",
      "trace_id": "trace-create-1"
    },
    "model": "gpt-4o-mini"
  }'
```

Send a message inside a session:
```bash
curl -X POST http://localhost:8080/v1/sessions/<session_id>/messages \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your-token-here" \
  -d '{
    "input": [
      { "type": "text", "text": "Hello, how are you?" }
    ],
    "stream": false
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
│  Gateway Client - Unified API Interface (talos-client)                    │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Gateway Layer - HTTP/WS API, Authentication, Rate Limiting               │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Orchestration Layer - Agent Coordination, Session Management, Routing     │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Cognition Layer - Memory, Knowledge, Reasoning, Decision Making           │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Action Layer - Tool Execution, Skill Execution, Output Generation         │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Perception Layer - Channels, Input Processing, Multi-modal Support        │
└─────────────────────────────────────────────────────────────────────────────┘
                                    ⬇
┌─────────────────────────────────────────────────────────────────────────────┐
│  Infrastructure Layer - Storage, Config, Logging, Error Handling           │
└─────────────────────────────────────────────────────────────────────────────┘
```

## 📁 Project Structure

- `crates/common/` - Shared utilities and LLM abstractions
- `crates/infra/` - Infrastructure components (config, logging, storage, auth)
- `crates/perception/` - Channel and input processing (includes ui-tui and channels)
- `crates/cognition/` - Memory, knowledge, reasoning systems
- `crates/action/` - Tools, skills, execution
- `crates/orchestration/` - Agent coordination, sessions, routing
- `crates/gateway/` - HTTP/WS gateways and client interfaces
- `bin/talos/` - Main binary executable

## ⚙️ Configuration Options

The system can be configured via `talos.yaml` or environment variables:

- `ANTHROPIC_API_KEY` - API key for Anthropic services
- `OPENAI_API_KEY` - API key for OpenAI services
- `BAILIAN_API_KEY` - API key for Bailian services
- `GATEWAY_TOKEN` - Authentication token for the API
- `TELEGRAM_BOT_TOKEN` - Token for Telegram bot integration
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
- Monitoring and observability features

## 🤝 Contributing

We welcome contributions! Please see our contributing guidelines for details on how to participate in the project.

## 📄 License

MIT License - see LICENSE file for details.

## 🎯 MVP Completion

✅ **MVP Version Completed** - Talos framework now includes all core functionality needed for an AI agent platform with multi-channel support, unified client interface, and extensible architecture.
