# Talos Terminal

[Chinese](README_zh.md)

`talos-tui` builds the public `talos` terminal binary. The binary contains both
interactive TUI mode and non-interactive command mode, backed by the same
embedded Talos runtime and Rust SDK adapter.

## Usage

Install the latest target-specific release bundle:

```bash
curl --proto '=https' --tlsv1.2 -fsSL \
  https://github.com/apilots/talos/releases/latest/download/install.sh | sh
```

The installer keeps immutable release resources outside `$TALOS_HOME`. See
[the installer documentation](../../../deploy/install/README.md) for version
pinning, paths, and installation E2E coverage.

```bash
# Open the interactive TUI. This is the default when no command is provided.
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos

# Explicitly open the interactive TUI.
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- tui

# Execute one task in command mode.
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- exec "hello"

# Reuse a session in command mode.
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- exec --session <session-id> "continue"

# List models and sessions.
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- models
cargo run --manifest-path talos-rs/Cargo.toml -p talos-tui --bin talos -- session list
```

## Command mode

Supported commands:

- `exec`: start one agent turn and render timeline output to stdout/stderr.
- `models`: list available models.
- `session list`: list recent sessions.
- `session info <session-id>`: show session metadata.
- `session set-model <session-id> <model>`: update the session model.
- `status`: show embedded runtime availability and basic counts.

Command mode intentionally reuses the TUI `GatewayClient` adapter for model,
session, and turn streaming behavior. It does not maintain a separate
standalone CLI runtime.

## Interactive TUI mode

The TUI supports:

- Chat and streaming timeline rendering.
- Model selection and model switching.
- Session history and session switching.
- Tool execution status panels.
- Slash commands such as `/models`, `/history`, `/sessions`, `/status`,
  `/compact`, `/clear`, and `/quit`.

## Runtime

The terminal binary runs the embedded Talos runtime in process. It does not
start an app-server child process and does not open a local port.
