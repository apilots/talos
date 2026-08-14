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

5. `scenarios/05_cli/`
- Scope: CLI-driven interactive scenarios
- Files:
  - `full_flow.sh`
  - `bilingual_chat.sh`

6. `scenarios/06_tools/`
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
  - `agent.sh`

7. `scenarios/07_memory/`
- Scope: memory and workspace docs
- Files:
  - `workspace_docs_api.sh`

8. `scenarios/08_tui/`
- Scope: embedded TUI lifecycle validation
- Files:
  - `embedded_session_controls.sh`
  - `embedded_write_html.sh`

9. `scenarios/09_app_server/`
- Scope: JSON-RPC app-server protocol, resource parity, skills, and MCP turn paths
- Files:
  - `protocol_e2e.sh`
  - `skills_e2e.sh`
  - `mcp_e2e.sh`

10. `scenarios/10_talos_app/`
- Scope: `talos-app` mock e2e and component/API regression tests
- Files:
  - `mock_e2e.sh`

11. `scenarios/11_auth/`
- Scope: Talos Cloud authentication integration
- Files:
  - `cloud_gateway.sh`

12. `scenarios/12_files/`
- Scope: user file lifecycle, full-size attachment wire encoding against a mock provider, and S3 object lifecycle
- Files:
  - `local_model_input.sh`
  - `s3_lifecycle.sh`

13. `scenarios/13_interaction/`
- Scope: durable product interaction through the real HTTP, Timeline, WebUI/Desktop, TUI, and UserAction paths
- Verifies: three-tool execution in every Session permission mode, real DeepSeek structured safe/risky assessment, ordered repeated approvals, identical concurrent approval submission with exactly-once execution, Question option and Other responses, same-Turn resume, refresh/reconnect recovery, and approval decisions
- Files:
  - `user_action_desktop.sh`
  - `user_action_http.sh`
  - `user_action_tui.sh`

14. `scenarios/14_sandbox/`
- Scope: unified tool sandbox acceptance through the real Registry dispatch path
- Runs cases natively on macOS/Linux; the full macOS group runs all cases once in the fixed Ubuntu 24.04 Docker image
- Files:
  - `01_provider_preflight.sh`
  - `02_process_runtime.sh`
  - `03_bash_terminal_status.sh`
  - `04_process_filesystem.sh`
  - `05_escape_attempts.sh`
  - `06_network_policy.sh`
  - `07_file_tools.sh`
  - `08_full_access_control.sh`
  - `09_approve_for_me_risk.sh`
  - `10_no_silent_fallback.sh`
  - `99_linux_docker.sh` (group-level Linux verification on macOS)

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
talos-e2e/run_e2e.sh --group 12_files
talos-e2e/run_e2e.sh --group 13_interaction
talos-e2e/run_e2e.sh --group 14_sandbox
```

From the repository root, the same runner is also exposed through `just`:

```bash
just e2e
just e2e-group 09_app_server
just e2e-case 09_app_server/skills_e2e
just e2e-group 14_sandbox
just e2e-case 14_sandbox/01_provider_preflight
```

The runner prebuilds only the binaries required by the selected scenario set.
Scenarios that call `start_server` build `talos-server`; CLI/TUI scenarios that
invoke the terminal client build `talos`. App-server scenarios call
`build_app_server_binaries` inside the scenario script when they need
`app-server` and `talos-app-server-test-client`.

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
cargo run -p talos-app-protocol --example generate_app_protocol_artifacts
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
