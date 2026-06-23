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
- `talos-sdk`: For the local JSON-RPC stdio client
- `app-server`: Embedded JSON-RPC stdio runtime
- `talos-memory`: For conversation history and context management
- `talos-tools`: For tool execution monitoring
- `ratatui` and `crossterm`: For cross-platform terminal rendering

## Usage

```bash
# Run the TUI application in embedded app-server mode (default, no port)
cargo run -p talos-ui-tui --bin talos

# Override embedded app-server binary
cargo run -p talos-ui-tui --bin talos -- --app-server-bin ./target/debug/app-server

# Environment variable alternatives
export TALOS_APP_SERVER_BIN=./target/debug/app-server
```

## Components

- `TuiApp`: Main application state and logic
- `AppState`: Enum defining different application states
- `run_tui_app`: Entry point for the TUI application
- UI rendering functions for different components and views

## Modes of Operation

### Embedded App-server Mode (Default)
- Starts `app-server` as a child process
- Uses JSON-RPC over stdio JSONL
- Does not listen on a local port
- Shares the same `talos-service::TalosService` path as HTTP and Desktop

Current app-server mode supports the main chat/session path, model listing,
session create/list/get/delete, active turn interrupt, model update, event
streaming, session turn history, and dynamic tool results.
