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

## Goal, Loop and Automation

`/goal` and `/loop` use the same command contract as the other clients. Enter or
Tab in slash suggestions selects a one-message mode. Send the objective in the
original composer, or submit `/goal <prompt>` or `/loop <prompt>` directly.
Both carry the existing System Skill selection through the normal Turn or queue
input. Alt+Backspace clears only
the mode and keeps the prompt. Successful send/queue consumes the mode; failures
keep the draft. To inspect a bare `/goal` or `/loop`, dismiss suggestions with Esc
and press Enter. Full commands such as `/loop 5m check CI` submit the unchanged `5m check CI` prompt with the selected System Skill. Slash selects a mode, not a schedule; the model interprets the request, and creation has no interval form. Named CLI `--every` / `--dynamic` flags directly build typed actions without composing slash text.

Ctrl+G expands the shared task/waiting list. Select a task, use Enter to
pause/resume a Goal or stop a Loop, `e` to edit, and `d` to clear. Goal edits keep
the existing token budget. Loop edits use Tab for fixed/model-decided mode and
Ctrl+Up/Down for integer minutes (minimum one); they affect subsequent execution.
The original Ctrl+Q queue actions remain: ordinary input during Goal/Loop execution
queues first, and explicit row Steer targets the same running Turn. Waiting user
actions keep their original admission rules.

`/automation` lists caller-owned tasks, `/automation ended` includes ended tasks,
and `/jobs COMPUTER_ID list|pause|resume|run-once [JOB_ID]` uses the actual
Computer/instance ownership API. Jobs are not model Tools or role-based UI controls.

Only an admitted Input with exact stored message identity gets a small Goal/Loop
footer. Later ordinary Steer messages do not. Native scrollback already printed by
the terminal is not rewritten or duplicated to add late metadata; normal history
reload restores its precise footer. New automatic Inputs read existing active
metadata as early as the original reconnect path permits.

Goal time spans creation to now, including pauses and later Turns; completed Goals freeze at their stable completion update timestamp. A Loop shows elapsed time only for its exact admitted Input in the current Turn, otherwise its next time or ended result. A model-decided Loop with no known next time says `model-decided`, which does not promise a scheduled wakeup. Fixed Loops have no uniform expiry; model-decided Loops may stop through their normal settlement. These are display projections, not client schedulers.

Completed, stopped and cleared tasks leave the composer panel automatically;
active or paused Goals and active Loops remain. Use `/goal` or `/automation ended`
to inspect completed records without keeping a task in the composer.

Named command equivalents require an existing Session:

```bash
talos goal --session SESSION_ID start "Fix the failing checks"
talos goal --session SESSION_ID edit "Include the new acceptance criterion"
talos goal --session SESSION_ID pause
talos goal --session SESSION_ID resume
talos loop --session SESSION_ID start --every 5 "Check CI"
talos loop --session SESSION_ID start --dynamic "Watch for meaningful changes"
talos loop --session SESSION_ID edit LOOP_ID --every 2 "Check CI and report changes"
talos loop --session SESSION_ID stop LOOP_ID
talos automation list --ended
talos automation jobs --computer COMPUTER_ID list
```

`goal start`, `goal resume` and `loop start` keep the existing embedded owner in
the foreground until the exact selected object is observed paused, complete,
stopped or removed. Creation uses the typed owner receipt, not prompt
matching. Keep the process running: persistent definitions do not imply offline
execution or a daemon. Other management commands update state; they do not
provide an execution host after process exit. Normal exit uses the original
client shutdown path; SIGINT keeps the original process termination behavior.
Native Job run-once uses the scheduler's existing shutdown/drain lifecycle.
