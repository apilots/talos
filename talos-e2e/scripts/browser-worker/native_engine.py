"""Run the real Desktop browser host with an owned, disposable Chrome endpoint."""

import json
from contextlib import contextmanager
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


from lifecycle import own_process

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


def require_free_loopback_port(port):
    # TIME_WAIT is safe to replace; an active listener still rejects this bind.
    # Never use SO_REUSEPORT or borrow an existing debugging endpoint.
    with socket.socket() as probe:
        probe.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        probe.bind(('127.0.0.1', port))


@contextmanager
def owned_chrome(report, profile_root, env):
    """Own a prepared Chrome process, profile and fixed private endpoint."""
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
    require_free_loopback_port(9222)
    # Keep Chrome's normal OS home for platform services; only this disposable
    # user-data-dir supplies its browser profile. Electron keeps its fixture home.
    chrome_env = dict(env, HOME=str(Path.home()))
    profile = Path(profile_root) / 'chrome'
    with (report / 'prepared-chrome.log').open('w') as log:
        process = subprocess.Popen([
            chrome, '--user-data-dir=' + str(profile),
            '--remote-debugging-port=9222', '--remote-debugging-address=127.0.0.1',
            '--enable-automation', '--no-first-run', '--no-default-browser-check',
            '--new-window', 'about:blank',
        ], env=chrome_env, stdout=log, stderr=log, start_new_session=True)
        own_process(process)
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
            (report / 'prepared-chrome.json').write_text(json.dumps({
                'pid': process.pid, 'version': version,
                'endpoint': 'http://127.0.0.1:9222', 'userDataDir': str(profile),
                'ownedTemporaryProfile': True,
            }, indent=2))
            print('Owned prepared Chrome:', version, flush=True)
            yield process
        finally:
            stop_owned(process)


def main():
    report = Path(os.environ.get('TALOS_SCENARIO_REPORT_DIR', ROOT / 'output/browser-worker'))
    report.mkdir(parents=True, exist_ok=True)
    result = report / 'native-engine-acceptance.json'
    result.unlink(missing_ok=True)
    electron = ROOT / 'talos-app/desktop/node_modules/electron/dist'
    electron /= 'Electron.app/Contents/MacOS/Electron' if sys.platform == 'darwin' else 'electron'
    env = {key: os.environ[key] for key in (
        'PATH', 'TMPDIR', 'DISPLAY', 'XAUTHORITY', 'XDG_RUNTIME_DIR', 'SYSTEMROOT',
    ) if key in os.environ}
    with tempfile.TemporaryDirectory(prefix='talos-native-browser-') as temporary:
        env.update(HOME=temporary, TALOS_ACCEPTANCE_CHROME='1',
                   TALOS_RESOURCE_DIR=str(ROOT),
                   TALOS_SCENARIO_REPORT_DIR=str(report))
        with owned_chrome(report, temporary, env):
            native = subprocess.Popen(
                [str(electron), str(Path(os.environ['TALOS_E2E_NATIVE_ROOT']) / 'dist')],
                env=env, start_new_session=True,
            )
            own_process(native)
            try:
                if native.wait(timeout=180):
                    raise RuntimeError('Desktop browser host acceptance failed')
            finally:
                stop_owned(native)
            checks = json.loads(result.read_text())
            assert checks and all(value is True for value in checks.values()), checks
            assert checks['downloadCollisionAndUuid'] and checks['debuggerDetachExplicitFailure']
            modes = json.loads((report / 'native-engine-modes.json').read_text())
            for mode in ('managed', 'connected'):
                assert modes[mode]['status'] == 'passed', modes
                assert (report / modes[mode]['pageScreenshot']).is_file(), modes
            print('CASE native_browser_host_owned_chrome_tabs_files_detach PASS', flush=True)


if __name__ == '__main__':
    main()
