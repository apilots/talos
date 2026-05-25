# Integration Test Scenarios

This directory contains end-to-end integration scenarios grouped by business flow.
Most scenarios are real server/client flows. The `10_talos_app` group covers
frontend and SDK mock-flow regression tests that do not start a real gateway.

## Scenario Groups

1. `scenarios/01_health_and_models.sh`
- Scope: server boot + health + models listing
- Verifies: `/health`, `/v1/models`, auth header path

2. `scenarios/02_session_lifecycle.sh`
- Scope: session CRUD-like lifecycle on native API
- Verifies: `/v1/sessions` create/get/list, `/v1/sessions/{id}` patch

3. `scenarios/03_messages_http_sse.sh`
- Scope: async ack + timeline event SSE + idempotency behavior
- Verifies: `/v1/sessions/{id}/messages`, `/v1/sessions/{id}/events`, `Idempotency-Key`

4. `scenarios/05_cli/`
- Scope: CLI-driven interactive scenarios
- Files:
  - `app_server_e2e.sh`
  - `full_flow.sh`
  - `bilingual_chat.sh`

5. `scenarios/06_tools/`
- Scope: tool-effect validation scenarios
- Files:
  - `http_bash.sh`
  - `http_memory_search.sh`
  - `cli_bash.sh`
  - `cli_memory_search.sh`
  - `read.sh`
  - `edit.sh`
  - `grep_glob.sh`
  - `write.sh`
  - `memory_get.sh`
  - `todo_write.sh`
  - `web_fetch.sh`
  - `web_search.sh`
  - `tool_search.sh`
  - `ask_user_question.sh`
  - `enter_plan_mode.sh`
  - `exit_plan_mode.sh`
  - `agent.sh`
  - `skill.sh`
  - `task_output.sh`
  - `task_stop.sh`

7. `scenarios/07_memory/`
- Scope: memory flush and workspace docs
- Files:
  - `session_close_flushes_memory.sh`
  - `workspace_docs_api.sh`

8. `scenarios/08_admin_llm.sh`
- Scope: user-scoped LLM config API
- Verifies: model/config read-write path for authenticated users

9. `scenarios/08_tui/`
- Scope: embedded TUI lifecycle validation
- Files:
  - `embedded_quit_flushes_memory.sh`

10. `scenarios/09_app_server/`
- Scope: JSON-RPC app-server protocol and resource parity
- Files:
  - `protocol_e2e.sh`

11. `scenarios/10_talos_app/`
- Scope: `talos-app` mock e2e and component/API regression tests
- Files:
  - `mock_e2e.sh`

## Runner

Use:

```bash
tests/integration/run_e2e.sh
tests/integration/run_e2e.sh --group 06_tools
tests/integration/run_e2e.sh --case 05_cli/full_flow
tests/integration/run_e2e.sh --case 05_cli/app_server_e2e
tests/integration/run_e2e.sh --case 09_app_server/protocol_e2e
tests/integration/run_e2e.sh --case 10_talos_app/mock_e2e
```

`protocol_e2e.sh` starts `app-server` through the Rust app-server
client and verifies initialize, model/config/session/event flows, replay,
multi-subscriber ordering, active-turn conflict handling, memory files,
workspace docs, project CRUD, and protocol error mapping.

Protocol fixtures are generated from Rust with:

```bash
cargo run -p talos-app-protocol --example export_fixtures
```

The command refreshes both the JSON Schema fixture and the generated TypeScript
protocol file used by `@talos/sdk-core`.

Useful environment variables for app-server scenarios:

- `TALOS_APP_SERVER_BIN`: App Server binary invoked directly
- `TALOS_LOCAL_LAUNCH_TOKEN`: local initialize token
- `TALOS_APP_SERVER_TEST_MODEL`: model override for turn smoke scenarios
- `TALOS_APP_SERVER_SCENARIO_TIMEOUT_SECS`: per-scenario timeout override

The runner generates:
- `tests/integration/reports/e2e_report_<timestamp>.md`
- `tests/integration/reports/latest_report.md`
- `tests/integration/reports/<group>/<case>/scenario.log`
- `tests/integration/reports/<group>/<case>/...` for scenario-specific stdout/stderr, stream logs, and server logs
