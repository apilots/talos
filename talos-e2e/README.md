# Integration Test Scenarios

This directory contains end-to-end integration scenarios grouped by business flow.
Most scenarios are real server/client flows. The `10_talos_app` group covers
frontend and SDK mock-flow regression tests that do not start a real gateway.

## Scenario Groups

1. `scenarios/01_health_and_models.sh`
- Scope: server boot + health + models listing
- Verifies: `/health`, `/v1/models`

2. `scenarios/02_session_lifecycle.sh`
- Scope: session CRUD-like lifecycle on native API
- Verifies: `/v1/sessions` create/get/list, `/v1/sessions/{id}` patch

3. `scenarios/03_messages_http_sse.sh`
- Scope: async ack + timeline event SSE + idempotency behavior
- Verifies: `/v1/sessions/{id}/messages`, `/v1/sessions/{id}/events`, `Idempotency-Key`

4. `scenarios/05_cli/`
- Scope: CLI-driven interactive scenarios
- Files:
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
  - `task_output.sh`
  - `task_stop.sh`

7. `scenarios/07_memory/`
- Scope: memory flush and workspace docs
- Files:
  - `session_close_flushes_memory.sh`
  - `workspace_docs_api.sh`

8. `scenarios/08_tui/`
- Scope: embedded TUI lifecycle validation
- Files:
  - `embedded_quit_flushes_memory.sh`
  - `embedded_write_html.sh`

9. `scenarios/09_app_server/`
- Scope: JSON-RPC app-server protocol, resource parity, skills, and MCP turn paths
- Files:
  - `protocol_e2e.sh`
  - `skills_e2e.sh`
  - `mcp_e2e.sh`

11. `scenarios/10_talos_app/`
- Scope: `talos-app` mock e2e and component/API regression tests
- Files:
  - `mock_e2e.sh`

## Runner

Use:

```bash
talos-e2e/run_e2e.sh
talos-e2e/run_e2e.sh --group 06_tools
talos-e2e/run_e2e.sh --group 09_app_server
talos-e2e/run_e2e.sh --case 05_cli/full_flow
talos-e2e/run_e2e.sh --case 09_app_server/protocol_e2e
talos-e2e/run_e2e.sh --case 09_app_server/skills_e2e
talos-e2e/run_e2e.sh --case 09_app_server/mcp_e2e
talos-e2e/run_e2e.sh --case 10_talos_app/mock_e2e
```

`protocol_e2e.sh` starts `app-server` through the Rust app-server
client and verifies initialize, model/config/session/event flows, replay,
multi-subscriber ordering, active-turn conflict handling, memory files,
workspace docs, project CRUD, and protocol error mapping.

`skills_e2e.sh` creates a local localizer fixture skill under the prepared E2E
`TALOS_HOME`, then validates `skills.suggest`, `skills.resolve`, `turn.start`,
and `event.subscribe` through the real app-server path. The turn asks the
selected skill to localize UI text with a glossary, and asserts the structured
translation output instead of a simple acknowledgement token.

`mcp_e2e.sh` creates a local Playwright MCP plugin source under the prepared
E2E `TALOS_HOME`, validates MCP plugin search/install/disable/enable/uninstall
through app-server JSON-RPC, configures Microsoft Playwright MCP at app-server
startup, then runs a real turn that must call Playwright MCP browser tools to
visit Hacker News and write a screenshot/report artifact.

Run the complete app-server group, including the MCP case, with:

```bash
talos-e2e/run_e2e.sh --group 09_app_server
```

Run only the MCP app-server case with:

```bash
talos-e2e/run_e2e.sh --case 09_app_server/mcp_e2e
```

Protocol fixtures are generated from Rust with:

```bash
cargo run -p talos-app-protocol --example export_fixtures
```

The command refreshes both the JSON Schema fixture and the generated TypeScript
protocol file used by `@talos/sdk-core`.

Useful environment variables for app-server scenarios:

- `TALOS_APP_SERVER_BIN`: App Server binary invoked directly
- `TALOS_APP_SERVER_TEST_MODEL`: model override for turn smoke scenarios
- `TALOS_APP_SERVER_SCENARIO_TIMEOUT_SECS`: per-scenario timeout override

The runner generates:
- `talos-e2e/reports/e2e_report_<timestamp>.md`
- `talos-e2e/reports/latest_report.md`
- `talos-e2e/reports/<group>/<case>/scenario.log`
- `talos-e2e/reports/<group>/<case>/...` for scenario-specific stdout/stderr, stream logs, and server logs

The runner uses `talos-e2e/.tmp/` as the default temporary root for generated
Talos homes and scenario scratch data. Override it with `TALOS_E2E_TMP_ROOT`
when a test run needs a different location.
