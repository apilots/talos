#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
for command in cargo docker node pnpm openssl python3; do require_command "${command}"; done
python3 -c 'import yaml, websockets'
export TALOS_E2E_WORKER_IMAGE="${TALOS_E2E_WORKER_IMAGE:-talos-worker:browser-e2e}"
docker image inspect "${TALOS_E2E_WORKER_IMAGE}" >/dev/null || {
  echo "Build the current standard Worker image first; see this group's README" >&2; exit 1;
}
python3 - "${TALOS_E2E_WORKER_IMAGE}" "${REPO_ROOT}" "${SCENARIO_REPORT_DIR}" <<'PY'
import hashlib, json, subprocess, sys
from pathlib import Path
image, root, report = sys.argv[1:]
driver = 'talos-rs/crates/action/browser/resources/browser_driver.py'
expected = hashlib.sha256((Path(root) / driver).read_bytes()).hexdigest()
actual = subprocess.check_output(['docker', 'run', '--rm', '--entrypoint', 'sha256sum',
    image, '/opt/talos/browser/browser_driver.py'], text=True).split()[0]
assert actual == expected, 'Build the current-source Worker image; packaged Browser driver differs'
identity = subprocess.check_output(['docker', 'image', 'inspect', '--format', '{{.Id}}', image], text=True).strip()
(Path(report) / 'worker-image.json').write_text(json.dumps({'image': identity, 'driverSha256': actual}, indent=2))
PY
if [[ -z "${TALOS_E2E_SERVER_BIN:-}" ]]; then
  build_server_binary
  export TALOS_E2E_SERVER_BIN="${TALOS_RS_DIR}/target/debug/talos-server"
fi
if [[ -z "${TALOS_E2E_APP_SERVER_BIN:-}" ]]; then
  (cd "${TALOS_RS_DIR}" && cargo build --locked -p app-server)
  export TALOS_E2E_APP_SERVER_BIN="${TALOS_RS_DIR}/target/debug/app-server"
fi
[[ -x "${TALOS_E2E_SERVER_BIN}" && -x "${TALOS_E2E_APP_SERVER_BIN}" ]] || { echo "Current Server and app-server binaries are required" >&2; exit 1; }
pnpm --dir "${TALOS_APP_DIR}/desktop" build:main
node "${TALOS_E2E_DIR}/scripts/browser-worker/build-native.mjs" media viewer
export TALOS_E2E_NATIVE_MEDIA=1
python3 "${TALOS_E2E_DIR}/scripts/browser-worker/fixture.py"
