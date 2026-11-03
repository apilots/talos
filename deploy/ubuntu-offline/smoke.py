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
if os.environ.get('MODEL_TEST') == '1':
    raise SystemExit('MODEL_TEST is not supported by the profile-free image smoke; use the standard provider E2E scenarios with explicit model credentials.')
repo = Path(__file__).resolve().parents[2]
arch = subprocess.check_output(['docker', 'image', 'inspect', image, '--format', '{{.Architecture}}'], text=True).strip()
output = Path(os.environ.get('OUTPUT_DIR', str(repo / 'output' / 'ubuntu-offline'))) / f'smoke-{arch}-{network}'
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
            if rendered_at and time.monotonic() - rendered_at > 2 and not quit_sent:
                os.write(master, b'\x03')  # Ctrl-C, the product's quit shortcut.
                quit_sent = True
            if process.poll() is not None:
                break
        process.wait(timeout=10)
        assert rendered_at and quit_sent and process.returncode == 0, \
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
    # Public isolated test fixture; the delivery image itself retains the original prod template.
    docker('run', '--rm', '--network', 'none', '--entrypoint', 'python3',
           '-v', f'{volume}:/home/talos/.talos', image, '-c', r'''
from pathlib import Path
import yaml
home = Path('/home/talos/.talos')
config = yaml.safe_load(Path('/opt/talos/conf/talos.yaml').read_text())
config['gateway'].update(port=19091, auth=False)
config['sync']['enabled'] = False
(home / 'conf').mkdir(parents=True, exist_ok=True)
(home / 'conf/talos.yaml').write_text(yaml.safe_dump(config, sort_keys=False))
''')
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
    docker('restart', name)
    ready()
    assert inside('cat', '/home/talos/.talos/acceptance-marker') == 'shared-volume'
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
