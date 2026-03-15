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

4. `scenarios/04_cli_full_flow.sh`
- Scope: CLI-driven full flow with running server
- Verifies: `models`, `session create/list/set-model`, `chat in`

5. `scenarios/05_chat_bilingual_and_tools.sh`
- Scope: bilingual chat + tool execution path
- Verifies: English response, Chinese response, `exec` tool call and JSON request/response log entries

## Runner

Use:

```bash
tests/integration/run_e2e.sh
```

The runner generates:
- `tests/integration/reports/e2e_report_<timestamp>.md`
- `tests/integration/reports/latest_report.md`
