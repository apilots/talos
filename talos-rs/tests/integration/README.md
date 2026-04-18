# Integration Test Scenarios

This directory contains end-to-end integration scenarios grouped by business flow.
All scenarios are real server/client flows and do not use mock gateway handlers.

## Scenario Groups

1. `scenarios/01_health_and_models.sh`
- Scope: server boot + health + models listing
- Verifies: `/health`, `/v1/models`, auth header path

2. `scenarios/02_session_lifecycle.sh`
- Scope: session CRUD-like lifecycle on native API
- Verifies: `/v1/sessions` create/get/list, `/v1/sessions/{id}` patch

3. `scenarios/03_messages_http_sse.sh`
- Scope: non-stream + SSE + idempotency behavior
- Verifies: `/v1/sessions/{id}/messages` (sync + stream), `Idempotency-Key`

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
  - `skill.sh`
  - `task_output.sh`
  - `task_stop.sh`

6. `scenarios/07_memory/`
- Scope: memory flush and workspace docs
- Files:
  - `session_close_flushes_memory.sh`
  - `workspace_docs_api.sh`

7. `scenarios/08_tui/`
- Scope: embedded TUI lifecycle validation
- Files:
  - `embedded_quit_flushes_memory.sh`

## Runner

Use:

```bash
tests/integration/run_e2e.sh
tests/integration/run_e2e.sh --group 06_tools
tests/integration/run_e2e.sh --case 05_cli/full_flow
```

The runner generates:
- `tests/integration/reports/e2e_report_<timestamp>.md`
- `tests/integration/reports/latest_report.md`
- `tests/integration/reports/<group>/<case>/scenario.log`
- `tests/integration/reports/<group>/<case>/...` for scenario-specific stdout/stderr, stream logs, and server logs
