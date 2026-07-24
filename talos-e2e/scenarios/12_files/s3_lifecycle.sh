#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export TALOS_TEST_PORT="${TALOS_FILES_S3_E2E_TALOS_PORT:-18083}"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"

require_command curl
require_command docker
require_command mc
require_command python3

TMP_DIR="$(make_e2e_temp_dir "talos-files-s3")"
CONFIG_TEMPLATE="${TMP_DIR}/talos.yaml"
FIXTURE_PATH="${REPO_ROOT}/build/desktop/icons/icon.png"
MINIO_CONFIG_DIR="${TMP_DIR}/mc"
MINIO_CONTAINER="talos-files-e2e-$$"
MINIO_IMAGE="${TALOS_FILES_E2E_MINIO_IMAGE:-minio/minio:RELEASE.2025-04-22T22-12-26Z}"
MINIO_ACCESS_KEY="talos-files-e2e"
MINIO_SECRET_KEY="talos-files-e2e-secret"

cleanup() {
  stop_server
  docker rm -f "${MINIO_CONTAINER}" >/dev/null 2>&1 || true
  if [[ "${TALOS_FILES_E2E_KEEP_TMP:-0}" != "1" ]]; then
    rm -rf "${TMP_DIR}"
  else
    echo "kept temp directory: ${TMP_DIR}"
  fi
}
trap cleanup EXIT

docker run -d --rm \
  --name "${MINIO_CONTAINER}" \
  -e "MINIO_ROOT_USER=${MINIO_ACCESS_KEY}" \
  -e "MINIO_ROOT_PASSWORD=${MINIO_SECRET_KEY}" \
  -p 127.0.0.1::9000 \
  "${MINIO_IMAGE}" server /data >/dev/null
MINIO_ADDRESS="$(docker port "${MINIO_CONTAINER}" 9000/tcp | head -n 1)"
MINIO_ENDPOINT="http://${MINIO_ADDRESS}"

for _ in $(seq 1 120); do
  if curl -fsS "${MINIO_ENDPOINT}/minio/health/live" >/dev/null 2>&1; then
    break
  fi
  sleep 0.25
done
curl -fsS "${MINIO_ENDPOINT}/minio/health/live" >/dev/null
mc alias set --config-dir "${MINIO_CONFIG_DIR}" files-e2e \
  "${MINIO_ENDPOINT}" "${MINIO_ACCESS_KEY}" "${MINIO_SECRET_KEY}" >/dev/null
mc mb --config-dir "${MINIO_CONFIG_DIR}" --ignore-existing files-e2e/talos-files >/dev/null
echo "CASE minio_start PASS image=${MINIO_IMAGE}"

BASE_CONFIG="${REPO_ROOT}/conf/talos-dev.yaml" \
CONFIG_TEMPLATE="${CONFIG_TEMPLATE}" \
MINIO_ENDPOINT="${MINIO_ENDPOINT}" \
MINIO_ACCESS_KEY="${MINIO_ACCESS_KEY}" \
MINIO_SECRET_KEY="${MINIO_SECRET_KEY}" \
python3 - <<'PY'
import json
import os
from pathlib import Path

text = Path(os.environ["BASE_CONFIG"]).read_text(encoding="utf-8")
start = text.find("storage:\n")
if start < 0:
    raise SystemExit("unable to locate storage configuration block")
storage = f'''storage:
  database_type: sqlite
  max_connections: 5
  files:
    default: "minio"
    backends:
      minio:
        type: "s3"
        root: "/talos-e2e"
        bucket: "talos-files"
        region: "us-east-1"
        endpoint: {json.dumps(os.environ["MINIO_ENDPOINT"])}
        access_key_id: {json.dumps(os.environ["MINIO_ACCESS_KEY"])}
        secret_access_key: {json.dumps(os.environ["MINIO_SECRET_KEY"])}
'''
Path(os.environ["CONFIG_TEMPLATE"]).write_text(
    text[:start] + storage,
    encoding="utf-8",
)
PY

export TALOS_HOME="${TMP_DIR}/home"
export TALOS_TEST_CONFIG_TEMPLATE="${CONFIG_TEMPLATE}"
start_server

upload_response="$(curl -sS -w $'\n%{http_code}' \
  -X POST "${GATEWAY_URL}/v1/files" \
  -F 'metadata={"filename":"fixture.png","scope":{"type":"personal"},"tags":["s3-e2e"],"metadata":{}};type=application/json' \
  -F "file=@${FIXTURE_PATH};type=application/octet-stream")"
upload_status="${upload_response##*$'\n'}"
upload_body="${upload_response%$'\n'*}"
file_id="$(json_get "${upload_body}" "file.fileId")"
if [[ "${upload_status}" != "200" || -z "${file_id}" ]]; then
  echo "CASE s3_file_upload FAIL status=${upload_status} body=${upload_body}"
  exit 1
fi
echo "CASE s3_file_upload PASS file_id=${file_id}"

list_body="$(curl -sS "${GATEWAY_URL}/v1/files?tag=s3-e2e")"
LIST_BODY="${list_body}" FILE_ID="${file_id}" python3 - <<'PY'
import json
import os

data = json.loads(os.environ["LIST_BODY"])
assert [item["fileId"] for item in data["data"]] == [os.environ["FILE_ID"]]
PY
echo "CASE s3_file_list PASS file_id=${file_id}"

object_path="files-e2e/talos-files/talos-e2e/${file_id}/content.png"
stored_fixture="${TMP_DIR}/stored.png"
mc cat --config-dir "${MINIO_CONFIG_DIR}" "${object_path}" >"${stored_fixture}"
if ! cmp -s "${FIXTURE_PATH}" "${stored_fixture}"; then
  echo "CASE s3_object_content FAIL object=${object_path}"
  exit 1
fi
if find "${TALOS_HOME}/data/files" -type f -print -quit 2>/dev/null | grep -q .; then
  echo "CASE s3_object_content FAIL unexpected_local_object=true"
  exit 1
fi
echo "CASE s3_object_content PASS object=${object_path}"

delete_body="$(curl -sS -X DELETE "${GATEWAY_URL}/v1/files/${file_id}")"
if [[ "$(json_get "${delete_body}" "fileId")" != "${file_id}" ]]; then
  echo "CASE s3_file_delete FAIL body=${delete_body}"
  exit 1
fi
if mc stat --config-dir "${MINIO_CONFIG_DIR}" "${object_path}" >/dev/null 2>&1; then
  echo "CASE s3_file_delete FAIL object_still_exists=${object_path}"
  exit 1
fi
echo "CASE s3_file_delete PASS file_id=${file_id}"
