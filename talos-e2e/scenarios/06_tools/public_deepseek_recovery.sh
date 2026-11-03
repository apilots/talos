#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

if [[ -z "${TALOS_LLM_DEEPSEEK_API_KEY:-}" ]]; then
  echo "CASE public_deepseek_precheck FAIL missing TALOS_LLM_DEEPSEEK_API_KEY"
  exit 1
fi

case_dir="$(make_e2e_temp_dir public-deepseek)"
export TALOS_HOME="${case_dir}/home"
cleanup() { stop_server; rm -rf "${case_dir}"; }
trap cleanup EXIT
start_server
mkdir -p "${case_dir}/workspace"
export CASE_WORKSPACE="${case_dir}/workspace"
export GATEWAY_URL SCENARIO_REPORT_DIR
session_id="$(new_session_id)"

python3 - "${TALOS_HOME}/conf/talos.yaml" <<'PY'
import json
import os
import secrets
import sys
from pathlib import Path
import yaml

config = yaml.safe_load(Path(sys.argv[1]).read_text())
assert config["llm"]["deepseek"]["base_url"].rstrip("/") == "https://api.deepseek.com"
workspace = Path(os.environ["CASE_WORKSPACE"])
(workspace / "source.txt").write_text(secrets.token_hex(16) + "\n")
prompts = [
    "Execute these tool steps in order. First use Bash to cat source.txt. "
    "Then use Read on deliberately-missing.txt; this file deliberately does not exist. "
    "After that tool error, recover using Bash with command "
    "'cat source.txt > recovered.txt && cat recovered.txt'. Do not create the missing file. "
    "Finish with RECOVERY_OK and the token actually read from source.txt.",
    "Continue the same session. Use Bash with command "
    "\"printf 'FOLLOWUP_OK\\n' >> recovered.txt && cat recovered.txt\". "
    "Finish with FOLLOWUP_OK. Do not call any other tool.",
]
for index, prompt in enumerate(prompts, 1):
    payload = {
        "nodeId": "embedded",
        "currentDirectory": str(workspace),
        "userPermissionMode": "fullAccess",
        "modelSelection": {"modelRef": {"provider": "deepseek", "id": "deepseek-v4-flash"}},
        "input": {"content": [{"type": "text", "text": prompt}]},
    }
    (Path(os.environ["SCENARIO_REPORT_DIR"]) / f"public-deepseek-{index}-request.json").write_text(json.dumps(payload))
PY

for step in 1 2; do
  payload="$(cat "${SCENARIO_REPORT_DIR}/public-deepseek-${step}-request.json")"
  send_message_response "${session_id}" "${payload}" "public-deepseek-${step}" \
    >"${SCENARIO_REPORT_DIR}/public-deepseek-${step}.json"
  python3 - "${step}" <<'PY'
import os
import sys
from pathlib import Path

workspace = Path(os.environ["CASE_WORKSPACE"])
expected = (workspace / "source.txt").read_text() + ("FOLLOWUP_OK\n" if sys.argv[1] == "2" else "")
assert (workspace / "recovered.txt").read_text() == expected
assert not (workspace / "deliberately-missing.txt").exists()
PY
done

python3 - "${session_id}" <<'PY'
import json
import os
import sys
import urllib.request
from pathlib import Path

report = Path(os.environ["SCENARIO_REPORT_DIR"])
workspace = Path(os.environ["CASE_WORKSPACE"])
source = (workspace / "source.txt").read_text()
history_url = os.environ["GATEWAY_URL"] + "/v1/sessions/" + sys.argv[1] + "/history?limit=10"
history = json.load(urllib.request.urlopen(history_url, timeout=30))
(report / "public-deepseek-history.json").write_text(json.dumps(history))
turns = history["data"]["data"]
assert len(turns) == 2 and all(t["result"]["status"] == "completed" for t in turns)
checks = []
for step in (1, 2):
    response = json.loads((report / f"public-deepseek-{step}.json").read_text())
    assert response["stop_reason"] == "end_turn", response["stop_reason"]
    assert response["session_id"] == sys.argv[1]
    calls = [x for x in response["output"] if x["type"] == "tool_call"]
    results = {x["call_id"]: x for x in response["output"] if x["type"] == "tool_result"}
    assert len({c["call_id"] for c in calls}) == len(calls)
    assert set(results) == {c["call_id"] for c in calls}
    successful_bash = [
        i for i, c in enumerate(calls)
        if c["name"] == "Bash" and not results[c["call_id"]]["is_error"]
    ]
    assert successful_bash
    if step == 1:
        failed_reads = [
            i for i, c in enumerate(calls)
            if c["name"] == "Read" and results[c["call_id"]]["is_error"]
            and isinstance(c["arguments"], dict)
            and (workspace / c["arguments"].get("path", "")).resolve()
            == (workspace / "deliberately-missing.txt").resolve()
        ]
    answer = "".join(x["text"] for x in response["output"] if x["type"] == "output_text")
    assert ("RECOVERY_OK" in answer and source.strip() in answer) if step == 1 else "FOLLOWUP_OK" in answer
    records = [json.loads(line[6:]) for line in (report / f"public-deepseek-{step}_events.log").read_text().splitlines() if line.startswith("data: ")]
    events = [r["event"]["data"] for r in records]
    assert any(e["payload"]["type"] == "messageBlockDelta" for e in events)
    turn_id = events[0]["identity"]["turnId"]
    request = urllib.request.Request(os.environ["GATEWAY_URL"] + "/v1/trajectory/turn/read", data=json.dumps({"identity": {"sessionId": sys.argv[1], "turnId": turn_id}, "limit": 128}).encode(), headers={"Content-Type": "application/json"})
    trace = json.load(urllib.request.urlopen(request, timeout=30))
    (report / f"public-deepseek-{step}-trace.json").write_text(json.dumps(trace))
    requests = [x["request"] for x in trace["data"]["data"] if x["type"] == "modelRequest"]
    assert requests and all(r["connectionId"] == "deepseek" and r["modelId"] == "deepseek-v4-flash" for r in requests)
    assert all(c["observedState"] == "completed" for r in requests for c in r["callRecords"])
    if step == 1:
        ordinals = {r["modelRequestId"]: r["ordinal"] for r in requests}
        tool_ordinals = {
            x["tool"]["callId"]: ordinals[x["tool"]["sourceModelRequestId"]]
            for x in trace["data"]["data"] if x["type"] == "tool"
        }
        assert any(
            successful_bash[0] < failed < recovered
            and tool_ordinals[calls[failed]["call_id"]] < tool_ordinals[calls[recovered]["call_id"]]
            for failed in failed_reads for recovered in successful_bash
        )
    checks.append({"step": step, "tool_names": [c["name"] for c in calls], "tool_errors": sum(results[c["call_id"]]["is_error"] for c in calls), "metrics": response["metrics"]})
(report / "public-deepseek-summary.json").write_text(json.dumps({"endpoint": "https://api.deepseek.com", "model": "deepseek-v4-flash", "sessionId": sys.argv[1], "checks": checks, "file_effect_verified": True}, indent=2))
print("CASE public_deepseek_recovery PASS real_public_model=true tool_failure_recovered=true same_session_followup=true file_effect=true")
PY
