#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export TALOS_TEST_PORT="${TALOS_FILES_E2E_TALOS_PORT:-18082}"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command python3

TMP_DIR="$(make_e2e_temp_dir "talos-files-local")"
CONFIG_TEMPLATE="${TMP_DIR}/talos.yaml"
FIXTURE_PATH="${REPO_ROOT}/build/desktop/icons/icon.png"
MOCK_LLM_PORT="${TALOS_FILES_E2E_MOCK_PORT:-18982}"
MOCK_LLM_PID=""
MOCK_REQUEST_LOG="$(scenario_report_path "mock_llm_requests.jsonl")"
MOCK_SERVER_LOG="$(scenario_report_path "mock_llm_server.log")"
: >"${MOCK_REQUEST_LOG}"

cleanup() {
  stop_server
  if [[ -n "${MOCK_LLM_PID}" ]] && kill -0 "${MOCK_LLM_PID}" 2>/dev/null; then
    kill "${MOCK_LLM_PID}" 2>/dev/null || true
    wait "${MOCK_LLM_PID}" 2>/dev/null || true
  fi
  if [[ "${TALOS_FILES_E2E_KEEP_TMP:-0}" != "1" ]]; then
    rm -rf "${TMP_DIR}"
  else
    echo "kept temp directory: ${TMP_DIR}"
  fi
}
trap cleanup EXIT

BASE_CONFIG="${REPO_ROOT}/conf/talos-dev.yaml" \
CONFIG_TEMPLATE="${CONFIG_TEMPLATE}" \
MOCK_BASE_URL="http://127.0.0.1:${MOCK_LLM_PORT}/v1" \
python3 - <<'PY'
import json
import os
from pathlib import Path

source = Path(os.environ["BASE_CONFIG"])
target = Path(os.environ["CONFIG_TEMPLATE"])
lines = source.read_text(encoding="utf-8").splitlines(keepends=True)
in_llm = False
in_openai = False
updated = {"provider": False, "model": False, "base_url": False}

for index, line in enumerate(lines):
    stripped = line.strip()
    indent = len(line) - len(line.lstrip(" "))
    if indent == 0:
        in_llm = stripped == "llm:"
        in_openai = False
        continue
    if in_llm and indent == 2:
        in_openai = stripped == "openai:"
        if stripped.startswith("provider:"):
            lines[index] = "  provider: openai\n"
            updated["provider"] = True
        elif stripped.startswith("model:"):
            lines[index] = "  model: gpt-5.4-mini\n"
            updated["model"] = True
        continue
    if in_openai and indent == 4 and stripped.startswith("base_url:"):
        lines[index] = f"    base_url: {json.dumps(os.environ['MOCK_BASE_URL'])}\n"
        updated["base_url"] = True

if not all(updated.values()):
    raise SystemExit(f"failed to configure mock OpenAI provider: {updated}")
target.write_text("".join(lines), encoding="utf-8")
PY

MOCK_LLM_PORT="${MOCK_LLM_PORT}" MOCK_REQUEST_LOG="${MOCK_REQUEST_LOG}" \
python3 -u - >"${MOCK_SERVER_LOG}" 2>&1 <<'PY' &
import json
import os
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

port = int(os.environ["MOCK_LLM_PORT"])
request_log = os.environ["MOCK_REQUEST_LOG"]

class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Length", "2")
        self.end_headers()
        self.wfile.write(b"ok")

    def do_POST(self):
        length = int(self.headers.get("Content-Length", "0"))
        payload = json.loads(self.rfile.read(length))
        with open(request_log, "a", encoding="utf-8") as handle:
            handle.write(json.dumps(payload, separators=(",", ":")) + "\n")
        body = (
            'data: {"choices":[{"index":0,"delta":{"content":"FILES_E2E_OK"}}]}\n\n'
            'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],'
            '"usage":{"prompt_tokens":2,"completion_tokens":1,"total_tokens":3}}\n\n'
            'data: [DONE]\n\n'
        ).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, _format, *_args):
        return

ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
PY
MOCK_LLM_PID="$!"

for _ in $(seq 1 40); do
  if curl -fsS "http://127.0.0.1:${MOCK_LLM_PORT}/health" >/dev/null 2>&1; then
    break
  fi
  if ! kill -0 "${MOCK_LLM_PID}" 2>/dev/null; then
    echo "CASE mock_llm_start FAIL log=${MOCK_SERVER_LOG}"
    exit 1
  fi
  sleep 0.25
done
curl -fsS "http://127.0.0.1:${MOCK_LLM_PORT}/health" >/dev/null
echo "CASE mock_llm_start PASS"

export TALOS_HOME="${TMP_DIR}/home"
export TALOS_TEST_CONFIG_TEMPLATE="${CONFIG_TEMPLATE}"
export TALOS_LLM_OPENAI_API_KEY="files-e2e-key"
start_server

upload_response="$(curl -sS -w $'\n%{http_code}' \
  -X POST "${GATEWAY_URL}/v1/files" \
  -F 'metadata={"filename":"fixture.png","scope":{"type":"personal"},"tags":["e2e"],"metadata":{"source":"local-e2e"}};type=application/json' \
  -F "file=@${FIXTURE_PATH};type=application/octet-stream")"
upload_status="${upload_response##*$'\n'}"
upload_body="${upload_response%$'\n'*}"
file_id="$(api_json_get "${upload_body}" "file.fileId")"
if [[ "${upload_status}" != "200" || -z "${file_id}" ]]; then
  echo "CASE local_file_upload FAIL status=${upload_status} body=${upload_body}"
  exit 1
fi
echo "CASE local_file_upload PASS file_id=${file_id}"

list_body="$(curl -sS "${GATEWAY_URL}/v1/files?scope=personal&tag=e2e&limit=1")"
LIST_BODY="${list_body}" FILE_ID="${file_id}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["LIST_BODY"])
if not (isinstance(data, dict) and data.get("status") == "success" and isinstance(data.get("data"), dict)):
    raise SystemExit("file list response must be ApiResponse success envelope")
data = data["data"]
assert [item["fileId"] for item in data["data"]] == [os.environ["FILE_ID"]]
PY
echo "CASE local_file_list PASS file_id=${file_id}"

read_body="$(curl -sS "${GATEWAY_URL}/v1/files/${file_id}")"
if [[ "$(api_json_get "${read_body}" "file.fileId")" != "${file_id}" ]]; then
  echo "CASE local_file_read FAIL body=${read_body}"
  exit 1
fi
echo "CASE local_file_read PASS file_id=${file_id}"

update_body="$(curl -sS -X PATCH "${GATEWAY_URL}/v1/files/${file_id}" \
  -H "Content-Type: application/json" \
  -d '{"tags":["reviewed"],"metadata":{"source":"model-e2e"}}')"
UPDATE_BODY="${update_body}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["UPDATE_BODY"])
if not (isinstance(data, dict) and data.get("status") == "success" and isinstance(data.get("data"), dict)):
    raise SystemExit("file update response must be ApiResponse success envelope")
data = data["data"]
file = data["file"]
assert file["tags"] == ["reviewed"]
assert file["metadata"] == {"source": "model-e2e"}
PY
echo "CASE local_file_update PASS file_id=${file_id}"

session_id="$(new_session_id)"
create_payload='{}'
remember_session_start_defaults "${session_id}" "${create_payload}"
turn_payload="$(FILE_ID="${file_id}" python3 - <<'PY'
import json
import os

print(json.dumps({
    "input": {
        "content": [
            {"type": "text", "text": "Inspect the attached image."},
            {"type": "file", "fileId": os.environ["FILE_ID"]},
        ]
    },
}, separators=(",", ":")))
PY
)"
turn_body="$(send_message_response "${session_id}" "${turn_payload}" "files_model_input")"
TURN_BODY="${turn_body}" python3 - <<'PY'
import json
import os

response = json.loads(os.environ["TURN_BODY"])
assert response["stop_reason"] != "error"
assert any(item.get("text") == "FILES_E2E_OK" for item in response.get("output", []))
PY
echo "CASE local_mock_attachment_turn PASS session_id=${session_id}"

MOCK_REQUEST_LOG="${MOCK_REQUEST_LOG}" FIXTURE_PATH="${FIXTURE_PATH}" python3 - <<'PY'
import base64
import json
import os
from pathlib import Path

expected = Path(os.environ["FIXTURE_PATH"]).read_bytes()
assert len(expected) > 64 * 1024
requests = [
    json.loads(line)
    for line in Path(os.environ["MOCK_REQUEST_LOG"]).read_text(encoding="utf-8").splitlines()
    if line.strip()
]
matched = False
for request in requests:
    for message in request.get("messages", []):
        content = message.get("content")
        if message.get("role") != "user" or not isinstance(content, list):
            continue
        types = [block.get("type") for block in content]
        if types != ["text", "image_url"]:
            continue
        image_url = content[1].get("image_url", {}).get("url")
        prefix = "data:image/png;base64,"
        if (
            isinstance(image_url, str)
            and image_url.startswith(prefix)
            and base64.b64decode(image_url[len(prefix):], validate=True) == expected
        ):
            matched = True
            break
    if matched:
        break
assert matched, requests
PY
echo "CASE local_mock_wire_attachment PASS"

delete_body="$(curl -sS -X DELETE "${GATEWAY_URL}/v1/files/${file_id}")"
if [[ "$(api_json_get "${delete_body}" "fileId")" != "${file_id}" ]]; then
  echo "CASE local_file_delete FAIL body=${delete_body}"
  exit 1
fi
read_after_delete_status="$(curl -sS -o /dev/null -w '%{http_code}' "${GATEWAY_URL}/v1/files/${file_id}")"
local_object_count="$(find "${TALOS_HOME}/data/files" -type f 2>/dev/null | wc -l | tr -d ' ')"
if [[ "${read_after_delete_status}" != "404" || "${local_object_count}" != "0" ]]; then
  echo "CASE local_file_delete FAIL status=${read_after_delete_status} objects=${local_object_count}"
  exit 1
fi
echo "CASE local_file_delete PASS file_id=${file_id}"
