#!/usr/bin/env python3
"""Exercise the packaged product through Docker, HTTP, CLI and a real terminal."""

import fcntl
import json
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import sys
import termios
import time
import uuid
import urllib.request

image = sys.argv[1]
name = 'talos-pre-smoke-' + uuid.uuid4().hex[:10]
volume = name + '-data'
network = os.environ.get('NETWORK', 'none')
model_test = os.environ.get('MODEL_TEST') == '1'
repo = Path(__file__).resolve().parents[2]
arch = subprocess.check_output(['docker', 'image', 'inspect', image, '--format', '{{.Architecture}}'], text=True).strip()
output = Path(os.environ.get('OUTPUT_DIR', str(repo / 'output' / 'ubuntu-server'))) / f'smoke-{arch}-{network}'
output.mkdir(parents=True, exist_ok=True)
results = []


def docker(*args, timeout=90, check=True):
    return subprocess.run(['docker', *args], text=True, stdout=subprocess.PIPE,
                          stderr=subprocess.STDOUT, timeout=timeout, check=check).stdout


def record(case, detail, status='PASS'):
    results.append({'case': case, 'status': status, 'detail': detail})
    print(f'{status} {case}: {detail}', flush=True)


def inside(*args, **kwargs):
    return docker('exec', name, *args, **kwargs)


def get(path):
    raw = inside('curl', '-fsS', '--max-time', '10', 'http://127.0.0.1:19091' + path)
    return json.loads(raw)


def ready():
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        state = docker('inspect', name, '--format', '{{.State.Status}} {{if .State.Health}}{{.State.Health.Status}}{{end}}')
        if 'healthy' in state and 'unhealthy' not in state:
            return
        if 'exited' in state or 'unhealthy' in state:
            raise AssertionError(f'Container failed: {state}')
        time.sleep(2)
    raise TimeoutError('Container health did not become ready')


def save_models_after_tui():
    config = get('/v1/llm?nodeId=embedded')['data']['config']
    config['bailian']['models'][0]['name'] = 'Shared TUI model save acceptance'
    for node in ('embedded', None):
        payload = json.dumps(config)
        target = '?nodeId=' + node if node else ''
        request = subprocess.run(
            ['docker', 'exec', '-i', name, 'curl', '--fail-with-body', '-sS', '--max-time', '30',
             '-X', 'PUT', '-H', 'Content-Type: application/json', '--data-binary', '@-',
             'http://127.0.0.1:19091/v1/llm' + target],
            input=payload, text=True, capture_output=True, check=True,
        )
        assert json.loads(request.stdout)['status'] == 'success'
    assert get('/v1/llm?nodeId=embedded')['data']['config']['bailian']['models'][0]['name'] == config['bailian']['models'][0]['name']
    record('model-save-after-tui', 'Bailian edit saved after CLI and real TUI startup, with embedded and default targets')


def tui():
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 36, 120, 0, 0))
    process = subprocess.Popen(['docker', 'exec', '-it', name, 'talos'],
                               stdin=slave, stdout=slave, stderr=slave, close_fds=True)
    os.close(slave)
    transcript = bytearray()
    deadline = time.monotonic() + 60
    rendered_at = None
    quit_sent = False
    prompt_sent = False
    reply_complete = not model_test
    previous_sessions = {s['id'] for s in get('/v1/sessions')['data']['sessions']} if model_test else set()
    next_history_check = 0
    try:
        while time.monotonic() < deadline:
            if select.select([master], [], [], 0.2)[0]:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    break
                if not data:
                    break
                transcript.extend(data)
                if b'\x1b[6n' in data:
                    os.write(master, b'\x1b[1;1R')
                if b'\x1b[?2004h' in transcript and b'Tip:' in transcript and len(transcript) > 300:
                    rendered_at = rendered_at or time.monotonic()
            if rendered_at and model_test and not prompt_sent:
                os.write(master, b'Reply with exactly UBUNTU_TUI_OK. Do not use tools.X\x7fY\x08\r')
                prompt_sent = True
            if prompt_sent and not reply_complete and time.monotonic() >= next_history_check:
                next_history_check = time.monotonic() + 1
                for session in get('/v1/sessions')['data']['sessions']:
                    if session['id'] in previous_sessions:
                        continue
                    history = get('/v1/sessions/' + session['id'] + '/history')
                    for turn in history['data']['data']:
                        for message in turn.get('result', {}).get('result', {}).get('messages', []):
                            for block in message['blocks']:
                                content = block['content']
                                if (content['type'] == 'assistantText' and
                                        content['data']['phase'] == 'finalAnswer' and
                                        content['data']['text'].strip() == 'UBUNTU_TUI_OK'):
                                    user_texts = [b['content']['data'].get('text')
                                                  for m in turn['result']['result']['messages'] if m['role'] == 'user'
                                                  for b in m['blocks'] if b['content']['type'] == 'userContent']
                                    assert user_texts == ['Reply with exactly UBUNTU_TUI_OK. Do not use tools.'], user_texts
                                    reply_complete = True
                                    (output / 'tui-history.json').write_text(json.dumps(history, indent=2))
                                    record('tui-model-shared-session', 'PTY DEL and Ctrl-H removed input correctly; actual assistant reply visible through HTTP')
            if rendered_at and reply_complete and time.monotonic() - rendered_at > 2 and not quit_sent:
                os.write(master, b'\x03')  # Ctrl-C, the product's quit shortcut.
                quit_sent = True
            if process.poll() is not None:
                break
        process.wait(timeout=10)
        assert rendered_at and reply_complete and quit_sent and process.returncode == 0, \
            f'TUI did not render and exit cleanly: exit={process.returncode}'
        assert b'Cloud login required' not in transcript
        (output / 'tui.ansi').write_bytes(transcript)
        record('tui', 'PTY rendered; Ctrl-C exited successfully without Cloud login')
    finally:
        (output / 'tui.ansi').write_bytes(transcript)
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
        os.close(master)


try:
    docker('volume', 'create', volume)
    publish = ['-p', '127.0.0.1::19091'] if network != 'none' else []
    docker('run', '-d', '--platform', f'linux/{arch}', '--name', name,
           '--network', network, '--shm-size=1g', '--security-opt', 'no-new-privileges:true',
           '--security-opt', 'seccomp=' + str(repo / 'deploy/docker/talos-worker/chromium-seccomp.json'),
           *publish, '-v', f'{volume}:/home/talos/.talos', image)
    ready()
    record('startup', f'healthy on linux/{arch}, network={network}')
    assert get('/health')['auth_enabled'] is False
    inside('python3', '-c', 'import pathlib, struct, sys; '
           'bins = ("talos", "talos-server", "talos-worker", "talos-linux-sandbox-launcher"); '
           'assert all(struct.unpack_from("<H", pathlib.Path("/opt/talos/bin", b).read_bytes(), 18)[0] '
           '== int(sys.argv[1]) for b in bins)', '183' if arch == 'arm64' else '62')
    record('architecture', f'all four ELF executables match linux/{arch}')
    if publish:
        port = docker('port', name, '19091/tcp').strip().rsplit(':', 1)[1]
        with urllib.request.urlopen(f'http://127.0.0.1:{port}/v1/user', timeout=10) as response:
            assert 'anonymous' in response.read().decode()
        record('published-port', 'host accesses anonymous API through Docker port mapping')
    inside('bash', '-euc', 'for binary in talos talos-server talos-worker talos-linux-sandbox-launcher; do '
           'ldd /opt/talos/bin/"$binary" > /tmp/ldd.txt; '
           '! grep -q "not found" /tmp/ldd.txt; done; '
           'talos --version; talos-server --version; talos-worker --version; '
           'bwrap --version')
    record('runtime-assets', 'all four binaries resolve shared libraries and version probes pass')
    probe = subprocess.run(['docker', 'exec', name, 'talos-linux-sandbox-launcher', '--probe-seccomp'],
                           text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=15)
    (output / 'seccomp.log').write_text(probe.stdout)
    if probe.returncode:
        host_arch = docker('info', '--format', '{{.Architecture}}').strip()
        host_arch = {'aarch64': 'arm64', 'x86_64': 'amd64'}.get(host_arch, host_arch)
        if host_arch != arch and 'Invalid argument (os error 22)' in probe.stdout:
            record('seccomp', f'{arch} emulation on {host_arch} rejects seccomp; native-host validation required', 'UNAVAILABLE')
        else:
            raise AssertionError(probe.stdout)
    else:
        record('seccomp', 'kernel accepted the sandbox filter')
    user = get('/v1/user')
    (output / 'user.json').write_text(json.dumps(user, indent=2))
    assert 'anonymous' in json.dumps(user), user
    record('anonymous', 'HTTP /v1/user works without credentials')
    html = inside('curl', '-fsS', 'http://127.0.0.1:19091/')
    assert '<html' in html.lower() and '/assets/' in html
    asset = __import__('re').search(r'(?:src|href)="(/assets/[^\"]+\.js)"', html)
    assert asset, 'WebUI JavaScript reference missing'
    inside('curl', '-fsS', '-o', '/dev/null', 'http://127.0.0.1:19091' + asset[1])
    record('webui', 'HTML and referenced JavaScript load from packaged resources')
    status = inside('talos', 'status')
    assert 'Talos runtime accessible' in status
    (output / 'cli-status.txt').write_text(status)
    models = json.loads(inside('talos', 'models', '--format', 'json'))
    assert models
    record('cli-shared-runtime', 'CLI reads models and status while Server runs with the same TALOS_HOME')
    tui()
    save_models_after_tui()
    inside('bash', '-c', 'test -s "$TALOS_HOME/data/talos.db" && '
           'test -s "$TALOS_HOME/conf/talos.yaml" && '
           'printf shared-volume > "$TALOS_HOME/acceptance-marker"')
    session_id = None
    if model_test:
        session_id = 'ubuntu-pre-' + uuid.uuid4().hex
        answer = inside('talos', 'exec', '--session', session_id,
                        'Reply with exactly UBUNTU_PRE_OK. Do not use any tools.', timeout=240)
        (output / 'model.txt').write_text(answer)
        assert 'UBUNTU_PRE_OK' in answer
        session = get('/v1/sessions/' + session_id)
        assert session_id in json.dumps(session)
        record('model-and-shared-session', 'CLI completed an internal model request; HTTP Server sees its session')
        tool_session = 'ubuntu-pre-tool-' + uuid.uuid4().hex
        tool_answer = inside('talos', 'exec', '--session', tool_session,
                             "You must call tool 'Bash' exactly once before any final answer. "
                             'Use command printf UBUNTU_PRE_TOOL_OK. Do not call any other tool. '
                             'Do not answer from prior knowledge or workspace context. '
                             'After the tool result is returned, return its raw stdout.', timeout=240)
        (output / 'tool.txt').write_text(tool_answer)
        assert 'TOOL_CALL_STARTED name=Bash' in tool_answer
        assert 'internal=Bash is_error=false' in tool_answer and 'UBUNTU_PRE_TOOL_OK' in tool_answer
        record('shell-tool', 'internal model called Bash successfully under the supplied pre policy')
    docker('restart', name)
    ready()
    assert inside('cat', '/home/talos/.talos/acceptance-marker') == 'shared-volume'
    if session_id:
        assert session_id in json.dumps(get('/v1/sessions/' + session_id))
    assert get('/v1/llm?nodeId=embedded')['data']['config']['bailian']['models'][0]['name'] == 'Shared TUI model save acceptance'
    record('model-save-persistence', 'Bailian model edit survives Server restart after shared TUI use')
    record('persistence', 'configuration, database and shared data survive container restart')
except Exception as error:
    if isinstance(error, subprocess.CalledProcessError):
        (output / 'command-failure.log').write_text(error.stdout or '')
    results.append({'case': 'acceptance', 'status': 'FAIL', 'detail': str(error)})
    raise
finally:
    (output / 'results.json').write_text(json.dumps(results, indent=2))
    (output / 'server.log').write_text(docker('logs', name, check=False))
    docker('rm', '-f', name, check=False)
    docker('volume', 'rm', volume, check=False)
