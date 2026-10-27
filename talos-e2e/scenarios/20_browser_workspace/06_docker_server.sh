#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
source "${SCRIPT_DIR}/../../scripts/common.sh"
export TALOS_SCENARIO_REPORT_DIR="${SCENARIO_REPORT_DIR}"
export TALOS_E2E_SERVER_IMAGE="${TALOS_E2E_SERVER_IMAGE:-talos-server:browser-e2e}"
require_command docker
require_command python3
python3 - "${REPO_ROOT}" "${SCENARIO_REPORT_DIR}" <<'PY'
import os
import json
from pathlib import Path
import subprocess
import sys
import uuid
import yaml
import websockets
from lifecycle import container_args

root, report = map(Path, sys.argv[1:])
viewer_package = Path(websockets.__file__).parent
report.mkdir(parents=True, exist_ok=True)
(report / 'default-config.json').write_text(json.dumps(yaml.safe_load((root / 'conf/talos-dev.yaml').read_text())))
# The packaged image intentionally runs as uid 10001; this directory belongs only to this case.
os.chmod(report, 0o777)
name = 'talos-server-browser-' + uuid.uuid4().hex[:10]
command = ['docker', 'run', *container_args(name), '--rm', '--network=none', '--shm-size=1g',
           '--security-opt', 'no-new-privileges:true', '--security-opt',
           'seccomp=' + str(root / 'deploy/docker/talos-worker/chromium-seccomp.json'),
           '-v', str(root) + ':/workspace:ro', '-v', str(report) + ':/report',
           '-v', str(viewer_package) + ':/fixture-deps/websockets:ro',
           '-e', 'PYTHONPATH=/fixture-deps:/workspace/talos-e2e/scripts',
           '-e', 'TALOS_SCENARIO_REPORT_DIR=/report', '-e', 'TALOS_E2E_TMP_ROOT=/tmp/server-e2e',
           '-e', 'TALOS_TUI_NO_BROWSER=1', os.environ['TALOS_E2E_SERVER_IMAGE'],
           'python3', '/workspace/talos-e2e/scripts/browser-worker/server.py']
with (report / 'acceptance.log').open('w') as log:
    result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT)
print((report / 'acceptance.log').read_text(), end='')
result.check_returncode()
PY
