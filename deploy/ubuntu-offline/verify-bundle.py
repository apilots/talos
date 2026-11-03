#!/usr/bin/env python3
"""Verify a delivery archive through Docker import and its packaged launcher."""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import time
import urllib.request
import uuid

from package import digest


def run(*args, **kwargs):
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT,
                                   timeout=300, **kwargs).strip()


def unpack(archive, destination):
    with tarfile.open(archive) as source:
        members = source.getmembers()
        names = [member.name for member in members]
        images = {f'talos-ubuntu-pre-{arch}.tar.gz' for arch in ('arm64', 'amd64')}
        allowed = images | {'chromium-seccomp.json', 'run.sh', 'image-prefix.txt',
                            'source-revision.txt', 'README.txt', 'SHA256SUMS-arm64',
                            'SHA256SUMS-amd64', 'SHA256SUMS-images'}
        if len(names) != len(set(names)) or not set(names) <= allowed:
            raise ValueError('Unexpected or duplicate delivery archive entries')
        for member in members:
            if not member.isfile():
                raise ValueError(f'Expected a regular archive member: {member.name}')
            with source.extractfile(member) as content, (destination / member.name).open('wb') as target:
                shutil.copyfileobj(content, target)
    architectures = [arch for arch in ('arm64', 'amd64')
                     if (destination / f'talos-ubuntu-pre-{arch}.tar.gz').is_file()]
    if not architectures:
        raise ValueError('No target images found in delivery archive')
    checksum = 'SHA256SUMS-images' if len(architectures) == 2 else f'SHA256SUMS-{architectures[0]}'
    checked = set()
    for line in (destination / checksum).read_text().splitlines():
        expected, filename = line.split(maxsplit=1)
        if filename in checked or filename not in names or digest(destination / filename) != expected:
            raise ValueError(f'Checksum mismatch or missing entry: {filename}')
        checked.add(filename)
    if checked != set(names) - {checksum}:
        raise ValueError('Checksum list does not cover every delivery member')
    return architectures


def wait_ready(name, expected_auth=False):
    port = run('docker', 'port', name, '8080/tcp').splitlines()[0].rsplit(':', 1)[1]
    url = f'http://127.0.0.1:{port}'
    for _ in range(90):
        try:
            with urllib.request.urlopen(url + '/health', timeout=3) as response:
                assert json.load(response)['auth_enabled'] is expected_auth
            with urllib.request.urlopen(url + '/', timeout=3) as response:
                assert '/assets/' in response.read().decode()
            return
        except (OSError, ValueError, AssertionError):
            time.sleep(2)
    raise RuntimeError(f'{name}: HTTP/WebUI on container port 8080 did not become ready')


def verify(directory, arch, output):
    name = 'talos-bundle-check-' + uuid.uuid4().hex[:10]
    volume = name + '-data'
    log = []
    try:
        log.append(run('docker', 'load', '-i', str(directory / f'talos-ubuntu-pre-{arch}.tar.gz')))
        prefix = (directory / 'image-prefix.txt').read_text().strip()
        revision = (directory / 'source-revision.txt').read_text().strip()
        metadata = json.loads(run('docker', 'image', 'inspect', prefix + '-' + arch,
                                 '--format', '{{json .}}'))
        assert metadata['Os'] == 'linux' and metadata['Architecture'] == arch
        assert metadata['Config']['Labels']['org.opencontainers.image.revision'] == revision
        assert prefix.endswith(':' + revision[:8])
        image = prefix + '-' + arch
        run('docker', 'run', '--rm', '--entrypoint', 'sh', image, '-ec',
            'test ! -e "$TALOS_HOME/conf/talos.yaml"')
        env = dict(os.environ, CONTAINER_NAME=name, DATA_VOLUME=volume, HTTP_PORT='127.0.0.1:0')
        log.append(run('bash', str(directory / 'run.sh'), arch, env=env))
        wait_ready(name, expected_auth=True)
        template_hash = run('docker', 'exec', name, 'sha256sum',
                            '/opt/talos/conf/talos.yaml').split()[0]
        run('docker', 'exec', name, 'bash', '-euc',
            'for tool in kubectl ssh scp sftp crontab curl wget dig nslookup ip ss '
            'ping traceroute nc ps top pgrep lsof strace tcpdump jq less vi rsync '
            'tar gzip unzip openssl git rg; do command -v "$tool" >/dev/null; done; '
            'kubectl version --client -o json > /tmp/kubectl-version.json')
        run('docker', 'rm', '-f', name)

        # Simulate an operator-owned config and an old cached template, never a private profile.
        fixture = {
            'deepseek': {'base_url': 'https://deepseek.example.invalid/v1',
                         'http_timeouts': {'connect_secs': 12, 'response_secs': 180,
                                           'stream_idle_secs': 150, 'stream_total_secs': 1200}},
            'openai': {'base_url': 'https://openai.example.invalid/v1',
                       'http_timeouts': {'connect_secs': 11, 'response_secs': 130,
                                         'stream_idle_secs': 100, 'stream_total_secs': 950}},
        }
        marker = uuid.uuid4().hex
        run('docker', 'run', '--rm', '--entrypoint', 'python3',
            '-v', volume + ':/home/talos/.talos', image, '-c', r'''
import json, sys
from pathlib import Path
import yaml
home = Path('/home/talos/.talos')
path = home / 'conf/talos.yaml'
config = yaml.safe_load(path.read_text())
config['gateway'].update(port=8080, auth=False)
config['sync']['enabled'] = False
for provider, settings in json.loads(sys.argv[1]).items():
    config['llm'][provider].update(settings)
config['llm']['deepseek']['models'][0].update(id='offline-upgrade-fixture', name='Offline upgrade fixture')
path.write_text(yaml.safe_dump(config, sort_keys=False))
(home / 'bundle-marker').write_text(sys.argv[2])
old_template = yaml.safe_load(Path('/opt/talos/conf/talos.yaml').read_text())
for provider in ('anthropic', 'openai', 'bailian', 'deepseek', 'openrouter', 'ollama'):
    old_template['llm'][provider].pop('http_timeouts', None)
(home / 'conf/talos.yaml.template').write_text(yaml.safe_dump(old_template, sort_keys=False))
''', json.dumps(fixture), marker)
        for _ in range(2):
            log.append(run('bash', str(directory / 'run.sh'), arch, env=env))
            wait_ready(name)
            run('docker', 'exec', name, 'python3', '-c', r'''
import json, sys
from pathlib import Path
import yaml
home = Path('/home/talos/.talos')
config = yaml.safe_load((home / 'conf/talos.yaml').read_text())
for provider, settings in json.loads(sys.argv[1]).items():
    for key, value in settings.items():
        assert config['llm'][provider][key] == value, (provider, key)
assert config['llm']['deepseek']['models'][0]['id'] == 'offline-upgrade-fixture'
assert config['llm']['deepseek']['models'][0]['name'] == 'Offline upgrade fixture'
assert config['llm']['ollama']['http_timeouts'] is None
assert config['gateway']['port'] == 8080 and config['gateway']['auth'] is False
assert (home / 'bundle-marker').read_text() == sys.argv[2]
assert (home / 'conf/talos.yaml.template').read_bytes() == Path('/opt/talos/conf/talos.yaml').read_bytes()
''', json.dumps(fixture), marker)
            run('docker', 'rm', '-f', name)
        return {'architecture': arch, 'status': 'PASS', 'revision': revision,
                'template_sha256': template_hash,
                'checks': ['archive checksums', 'docker load', 'image provenance',
                           'no baked runtime profile', 'prod bootstrap HTTP/WebUI',
                           'installed tools', 'cached template refresh',
                           'saved timeout/model settings retained across two recreations']}
    except subprocess.CalledProcessError as error:
        log.append(error.output or '')
        raise
    finally:
        logs = subprocess.run(['docker', 'logs', name], capture_output=True, text=True)
        if logs.returncode == 0:
            log.append(logs.stdout + logs.stderr)
        (output / f'bundle-{arch}.log').write_text('\n'.join(log) + '\n')
        subprocess.run(['docker', 'rm', '-f', name], capture_output=True)
        subprocess.run(['docker', 'volume', 'rm', volume], capture_output=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('--output-dir', type=Path, default=Path(__file__).resolve().parents[2]
                        / 'output/ubuntu-offline/acceptance')
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    results = []
    try:
        with tempfile.TemporaryDirectory(prefix='bundle-', dir=args.output_dir) as scratch:
            directory = Path(scratch).resolve()
            for arch in unpack(args.archive, directory):
                results.append(verify(directory, arch, args.output_dir))
                print(f'PASS bundle {arch}', flush=True)
    except Exception as error:
        results.append({'status': 'FAIL', 'detail': str(error)})
        raise
    finally:
        (args.output_dir / 'results.json').write_text(json.dumps(results, indent=2) + '\n')


if __name__ == '__main__':
    main()
