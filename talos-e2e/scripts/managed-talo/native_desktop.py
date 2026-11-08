#!/usr/bin/env python3
"""Open the real Electron shell and verify Server-origin Talo media."""
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlsplit

from playwright.sync_api import Error, sync_playwright

ROOT = Path(__file__).resolve().parents[3]
DESKTOP = ROOT / 'talos-app/desktop'


def port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def electron():
    default = DESKTOP / 'node_modules/electron/dist'
    native = (default / 'Electron.app/Contents/MacOS/Electron' if sys.platform == 'darwin'
              else default / 'electron')
    return Path(os.environ.get('TALOS_E2E_ELECTRON', native)).resolve(strict=True)


def capture(playwright, process, endpoint, base, node_id, report, stage):
    deadline = time.monotonic() + 15
    while True:
        try:
            browser = playwright.chromium.connect_over_cdp(endpoint, timeout=1200)
            break
        except Error:
            if process.poll() is not None or time.monotonic() >= deadline:
                raise
            time.sleep(.2)
    context = browser.contexts[0]
    page = context.pages[0] if context.pages else context.wait_for_event('page', timeout=15000)
    page.wait_for_url(lambda url: url.startswith(base + '/'), timeout=15000)
    streams, nodes = [], []

    def observe_request(request):
        if urlsplit(request.url).path == '/v1/desktop/open':
            nodes.append(request.post_data_json['nodeId'])

    page.on('request', observe_request)
    page.on('websocket', lambda ws: streams.append(urlsplit(ws.url)))
    assert page.evaluate('typeof window.talos') == 'undefined'
    electron_version = page.evaluate(r"navigator.userAgent.match(/Electron\/[\d.]+/)?.[0]")
    assert electron_version
    try:
        sign_in = page.get_by_role('button', name='Continue with Talos Cloud')
        sign_in.click(timeout=18000)
        sign_in.wait_for(state='hidden', timeout=38000)
        page.goto(base + '/talo')
        connect = page.get_by_role('button', name='Connect desktop', exact=True)
        connect.wait_for(timeout=22000)
        if stage == 'before-reset':
            page.screenshot(path=report / 'electron-talo-chooser.png')
        connect.click()
        page.wait_for_function('''() => [...document.querySelectorAll('canvas')].some(canvas => {
            if (canvas.width < 20 || canvas.height < 20) return false;
            const context = canvas.getContext('2d');
            if (!context) return false;
            const colors = new Set();
            for (let y = 0; y < 8; y++) for (let x = 0; x < 8; x++) {
                colors.add(String(context.getImageData(Math.floor(canvas.width*x/8), Math.floor(canvas.height*y/8), 1, 1).data));
            }
            return colors.size > 3;
        })''', polling=100, timeout=24000)
        media = [stream for stream in streams if stream.path == '/v1/desktop/stream']
        assert node_id in nodes and media and all(stream.netloc == urlsplit(base).netloc for stream in media)
        page.screenshot(path=report / f'electron-talo-{stage}.png')
        (report / f'electron-talo-{stage}.json').write_text(json.dumps({
            'stage': stage, 'electron': electron_version, 'nodeId': node_id, 'sameOriginDesktopRelay': True,
            'framebufferPixels': True, 'endpoints': [stream.path for stream in media],
        }, indent=2))
    except Exception:
        page.screenshot(path=report / f'electron-{stage}-failure.png', timeout=3000)
        raise


def main():
    if sys.argv[1:] == ['--check']:
        electron()
        return
    base, node_id, report, stage = sys.argv[1:]
    report = Path(report)
    debug_port = port()
    with tempfile.TemporaryDirectory(prefix='talos-electron-') as home, (report / f'electron-{stage}.log').open('wb') as log:
        process = subprocess.Popen([
            str(electron()), str(DESKTOP), f'--remote-debugging-port={debug_port}', f'--user-data-dir={home}',
        ], env={**os.environ, 'HOME': home, 'TALOS_DESKTOP_SERVER_URL': base, 'LANG': 'en_US.UTF-8'},
            stdout=log, stderr=log, start_new_session=True)
        try:
            with sync_playwright() as playwright:
                capture(playwright, process, f'http://127.0.0.1:{debug_port}', base, node_id, report, stage)
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait()


if __name__ == '__main__':
    main()
