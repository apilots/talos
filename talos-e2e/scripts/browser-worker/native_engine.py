"""Run the real Desktop browser host with an owned, disposable Chrome endpoint."""

import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import time
from urllib.request import urlopen


ROOT = Path(__file__).resolve().parents[3]


def stop_owned(process):
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        return
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=5)


def main():
    if sys.platform not in ('darwin', 'linux'):
        raise RuntimeError('Native browser acceptance requires macOS or Linux private IPC')
    chrome = os.environ.get('TALOS_E2E_CHROME_BIN')
    if not chrome:
        chrome = ('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
                  if sys.platform == 'darwin' else
                  shutil.which('google-chrome') or shutil.which('chromium'))
    if not chrome or not Path(chrome).is_file():
        raise RuntimeError('Set TALOS_E2E_CHROME_BIN to installed Chrome')
    # The prepared-Chrome adapter deliberately accepts only the local 9222 endpoint.
    # Never borrow an endpoint owned by the user or another test.
    with socket.socket() as probe:
        probe.bind(('127.0.0.1', 9222))
    report = Path(os.environ.get('TALOS_SCENARIO_REPORT_DIR', ROOT / 'output/browser-worker'))
    report.mkdir(parents=True, exist_ok=True)
    result = report / 'native-engine-acceptance.json'
    result.unlink(missing_ok=True)
    electron = ROOT / 'talos-app/desktop/node_modules/electron/dist'
    electron /= 'Electron.app/Contents/MacOS/Electron' if sys.platform == 'darwin' else 'electron'
    env = {key: os.environ[key] for key in (
        'PATH', 'TMPDIR', 'DISPLAY', 'XDG_RUNTIME_DIR', 'SYSTEMROOT',
    ) if key in os.environ}
    with tempfile.TemporaryDirectory(prefix='talos-native-browser-') as temporary:
        env.update(HOME=temporary, TALOS_ACCEPTANCE_CHROME='1',
                   TALOS_SCENARIO_REPORT_DIR=str(report))
        with (report / 'prepared-chrome.log').open('w') as log:
            process = subprocess.Popen([
                chrome, '--user-data-dir=' + str(Path(temporary) / 'chrome'),
                '--remote-debugging-port=9222', '--remote-debugging-address=127.0.0.1',
                '--enable-automation', '--no-first-run', '--no-default-browser-check',
                '--new-window', 'about:blank',
            ], env=env, stdout=log, stderr=log, start_new_session=True)
            try:
                deadline = time.monotonic() + 30
                while True:
                    if process.poll() is not None:
                        raise RuntimeError('Owned Chrome exited before CDP readiness')
                    try:
                        with urlopen('http://127.0.0.1:9222/json/version', timeout=1) as response:
                            version = json.load(response)['Browser']
                        break
                    except (OSError, ValueError):
                        if time.monotonic() >= deadline:
                            raise RuntimeError('Owned Chrome CDP readiness timed out')
                        time.sleep(0.1)
                print('Owned prepared Chrome:', version, flush=True)
                native = subprocess.Popen(
                    [str(electron), str(ROOT / '.tmp/browser-worker/native/dist')],
                    env=env, start_new_session=True,
                )
                try:
                    if native.wait(timeout=180):
                        raise RuntimeError('Desktop browser host acceptance failed')
                finally:
                    stop_owned(native)
                checks = json.loads(result.read_text())
                assert checks and all(value is True for value in checks.values()), checks
                assert checks['downloadCollisionAndUuid'] and checks['debuggerDetachExplicitFailure']
                print('CASE native_browser_host_owned_chrome_tabs_files_detach PASS', flush=True)
            finally:
                stop_owned(process)


if __name__ == '__main__':
    main()
