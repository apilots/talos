# Talos TUI (Terminal User Interface)

Talos TUI provides a terminal-based user interface for interacting with the Talos AI system. It offers a rich, interactive experience directly from the command line.

## Features

- **Multi-panel Interface**: Dedicated panels for chat history, tool execution status, and settings
- **Keyboard Navigation**: Full keyboard control with tab navigation between different views
- **Interactive Elements**: Support for message input, command execution, and settings configuration
- **Responsive Layout**: Adapts to different terminal sizes with a well-organized layout
- **Session Management**: Track conversation history and manage multiple sessions

## Views

The TUI supports four main views accessible via tab navigation:

1. **Chat View**: Main conversation interface with message history
2. **History View**: Browse past conversations and interactions
3. **Tools View**: Monitor and manage tool execution status
4. **Settings View**: Configure application settings and parameters

## Controls

- `Tab`: Switch between different application views
- `Enter`: Submit input in chat view
- `Arrow Keys`: Navigate lists and history
- `Ctrl+C`: Quit the application
- `Backspace`: Edit input in the input field

## Architecture

The TUI module integrates with:
- `talos-agent`: For AI processing and response generation
- `talos-memory`: For conversation history and context management
- `talos-tools`: For tool execution monitoring
- `ratatui` and `crossterm`: For cross-platform terminal rendering

## Usage

```bash
# Run the TUI application in direct mode (default)
cargo run -p talos-ui-tui

# Run the TUI application in gateway client mode
cargo run -p talos-ui-tui -- --gateway

# Override gateway endpoint and auth token (optional)
cargo run -p talos-ui-tui -- --gateway --gateway-url http://127.0.0.1:8080 --auth-token your-token

# Environment variable alternatives
export TALOS_GATEWAY_URL=http://127.0.0.1:8080
export GATEWAY_TOKEN=your-token
```

## Components

- `TuiApp`: Main application state and logic
- `AppState`: Enum defining different application states
- `run_tui_app`: Entry point for the TUI application
- UI rendering functions for different components and views

## Modes of Operation

### Direct Agent Mode (Default)
- Runs the full agent stack locally
- Does not require a separate server
- Good for testing and development

### Gateway Client Mode
- Connects to a remote Talos server
- Offloads computation to centralized service
- Better for production use with multiple clients
