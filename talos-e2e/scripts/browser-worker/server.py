"""Packaged Server and shared TUI regression, entirely inside an isolated container."""
import base64
import copy
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import shutil
import struct
import subprocess
import sys
import termios
import threading
import time
import urllib.request
import uuid
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import fixture

REPORT = fixture.REPORT
URL = 'http://127.0.0.1:8080'


def api(path, body=None, method=None):
    request = urllib.request.Request(URL + path, method=method,
        data=None if body is None else json.dumps(body).encode(),
        headers={'Content-Type': 'application/json'})
    with urllib.request.urlopen(request, timeout=180) as response:
        return fixture.data(json.load(response))


class Page(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def do_GET(self):
        content = b'''<!doctype html><title>Packaged Server browser acceptance</title>
<input name="name"><button onclick="document.querySelector('p').textContent='Hello '+document.querySelector('input').value">Submit</button><p>Ready</p>'''
        self.send_response(200)
        self.send_header('Content-Length', str(len(content)))
        self.end_headers()
        self.wfile.write(content)


def tui(env):
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 36, 120, 0, 0))
    process = subprocess.Popen(['talos'], env=env, stdin=slave, stdout=slave, stderr=slave)
    os.close(slave)
    transcript = bytearray()
    edited = False
    quit_sent = False
    try:
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            if select.select([master], [], [], .2)[0]:
                try:
                    chunk = os.read(master, 65536)
                except OSError:
                    break
                if not chunk:
                    break
                transcript.extend(chunk)
                if b'\x1b[6n' in chunk:
                    os.write(master, b'\x1b[1;1R')
            if not edited and b'Tip:' in transcript and b'\x1b[?2004h' in transcript:
                # DEL and BS must both delete before submitting a local command, with no model request.
                # Accept command completion, then execute the completed command.
                os.write(master, b'/helpX\x7fY\x08\r\r')
                edited = True
            if edited and not quit_sent and b'Commands:' in transcript:
                os.write(master, b'\x03')
                quit_sent = True
            if process.poll() is not None:
                break
        process.wait(timeout=10)
        assert edited and quit_sent and process.returncode == 0, 'TUI did not edit, run /help and exit'
        assert b'Unknown command' not in transcript
        assert not fixture.calls, 'Local TUI command unexpectedly called a model'
    finally:
        (REPORT / 'tui.ansi').write_bytes(transcript)
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
        os.close(master)


def browser_task():
    snapshot = api('/v1/browser', {'nodeId': 'embedded', 'operation': {'kind': 'read'}})
    assert snapshot['host'] == 'docker' and snapshot['enabled'], snapshot
    session = 'packaged-server-' + uuid.uuid4().hex
    ack = api('/v1/turns/start', {
        'sessionId': session, 'nodeId': 'embedded', 'userPermissionMode': 'fullAccess',
        'modelSelection': {'modelRef': {'provider': 'openai', 'id': 'browser-fixture'}, 'reasoningEffort': None},
        'input': {'content': [{'type': 'text', 'text': 'Verify the local browser page.'}]},
    })
    events = []
    with urllib.request.urlopen(URL + '/v1/sessions/' + session + '/events?limit=10000&turn_id=' + ack['turnId'], timeout=180) as response:
        for line in response:
            if not line.startswith(b'data: '):
                continue
            envelope = json.loads(line[6:])
            if envelope.get('event', {}).get('type') != 'agent':
                continue
            event = envelope['event']['data']
            events.append(event)
            if event['payload']['type'] in ('turnCompleted', 'turnFailed', 'turnCancelled'):
                break
    (REPORT / 'events.json').write_text(json.dumps(events, indent=2))
    assert events and events[-1]['payload']['type'] == 'turnCompleted', events[-1:] or 'No events'
    results = [event['payload']['data']['block']['content']['data'] for event in events
               if event['payload']['type'] == 'messageBlockCompleted'
               and event['payload']['data']['block']['content']['type'] == 'toolResult']
    browser = [result for result in results if 'browser_' in result['toolName']]
    assert len(browser) == 7 and not any(result['isError'] for result in browser), browser
    assert 'Hello Worker' in json.dumps(results)
    assert 'BROWSER_WORKER_ACCEPTANCE_OK' in json.dumps(events)
    images = [item for result in results for item in result['content'] if item['type'] == 'image']
    assert len(images) == 1 and images[0].get('fileId'), images
    content = api('/v1/files/' + images[0]['fileId'] + '/content')
    png = base64.b64decode(content['dataBase64'], validate=True)
    assert png.startswith(b'\x89PNG\r\n\x1a\n')
    (REPORT / 'browser.png').write_bytes(png)
    commands = []
    for process in Path('/proc').iterdir():
        if not process.name.isdigit():
            continue
        try:
            command = (process / 'cmdline').read_bytes().replace(b'\0', b' ').decode(errors='replace')
        except OSError:
            continue
        if '/chrome ' in command or '/chrome --' in command:
            assert '--no-sandbox' not in command
            commands.append(command)
    assert commands, 'Real Chromium process missing'
    (REPORT / 'chrome-commands.txt').write_text('\n'.join(commands))


def run():
    assert os.getuid() != 0, 'Acceptance must use the packaged non-root user'
    for command in ('talos-server', 'talos-worker', 'talos', 'talos-linux-sandbox-launcher',
                    'ssh', 'curl', 'dig', 'ip', 'ps', 'lsof', 'jq', 'vi'):
        assert shutil.which(command), 'Packaged command missing: ' + command
    model = ThreadingHTTPServer(('127.0.0.1', 0), fixture.Fixture)
    page = ThreadingHTTPServer(('127.0.0.1', 18080), Page)
    for service in (model, page):
        threading.Thread(target=service.serve_forever, daemon=True).start()
    home = Path(os.environ['HOME']) / '.talos'
    defaults = json.loads((REPORT / 'default-config.json').read_text())
    document = fixture.configure(defaults, home, 8080, model.server_port)
    # Keep the real pinned official source; the shared fixture disables sources for other consumers.
    document['agent']['kernel']['plugins']['sources'] = defaults['agent']['kernel']['plugins']['sources']
    document['gateway']['auth'] = False
    config = home / 'acceptance.json'
    config.write_text(json.dumps(document))
    env = dict(os.environ, TALOS_HOME=str(home), TALOS_CONFIG=str(config))
    process = None
    log = (REPORT / 'server.log').open('w')

    def start():
        nonlocal process
        process = subprocess.Popen(['talos-server'], env=env, stdout=log, stderr=subprocess.STDOUT)
        fixture.wait(lambda: process.poll() is None and api('/v1/user')['user']['user_id'] == 'anonymous',
                     'Anonymous packaged Server startup')

    try:
        start()
        with urllib.request.urlopen(URL, timeout=15) as response:
            assert '/assets/' in response.read().decode(), 'Packaged WebUI missing'
        subprocess.run(['talos', 'status'], env=env, check=True, capture_output=True, timeout=45)
        tui(env)
        llm = api('/v1/llm?nodeId=embedded')['config']
        llm['openai']['models'][0]['name'] = 'Edited after shared TUI'
        added = copy.deepcopy(llm['openai']['models'][0])
        added.update(id='shared-home-model', name='Added after shared TUI')
        llm['openai']['models'].append(added)
        # The HTTP API accepts the LlmConfig itself; Computer targeting is a query parameter.
        api('/v1/llm?nodeId=embedded', llm, method='PUT')
        assert api('/v1/llm?nodeId=embedded')['config'] == llm
        process.terminate()
        assert process.wait(timeout=30) == 0, 'Server did not stop cleanly'
        start()
        assert api('/v1/llm?nodeId=embedded')['config'] == llm, 'Model edit/add lost after Server restart'
        browser_task()
        (REPORT / 'results.json').write_text(json.dumps({
            'status': 'PASS', 'network': 'none', 'anonymous': True,
            'sharedCliAndPtyTui': True, 'modelEditAndAddPersisted': True,
            'restart': 'serverProcess', 'browserToolCalls': 7, 'sandboxDisabled': False,
        }, indent=2))
        print('PASS packaged anonymous Server / shared CLI + PTY TUI / model save + restart / sandboxed Browser MCP', flush=True)
    finally:
        if process is not None and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=10)
        log.close()
        for service in (page, model):
            service.shutdown()
            service.server_close()


if __name__ == '__main__':
    run()
