"""Owned resources for the existing E2E invocation; no global cleanup operations."""
import fcntl
import json
import os
from pathlib import Path
import signal
import shutil
import subprocess
import sys
import tempfile
import time

LABEL = 'love.talos.e2e.run'
KINDS = {'process', 'container', 'volume', 'network', 'image', 'directory'}


def root():
    return Path(os.environ['TALOS_E2E_RESOURCE_ROOT']).resolve()


def register(kind, value, scope=None):
    if kind not in KINDS:
        raise ValueError(f'Unknown resource kind: {kind}')
    scope = scope or os.environ['TALOS_E2E_CASE_ID']
    record = {'kind': kind, 'value': str(value), 'scope': scope}
    if kind == 'process':
        record['group'] = True
        record['started'] = subprocess.check_output(['ps', '-p', str(value), '-o', 'lstart='], text=True).strip()
    root().mkdir(parents=True, exist_ok=True)
    with (root() / 'resources.jsonl').open('a', encoding='utf-8') as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)
        stream.write(json.dumps(record) + '\n')
    return value


def own_process(process):
    """Register only explicitly detached process groups; shell descendants inherit case ownership."""
    try:
        if os.getpgid(process.pid) != process.pid:
            raise ValueError('Only detached process groups may be registered')
        register('process', process.pid)
    except (ProcessLookupError, subprocess.CalledProcessError):
        # A short command can finish before its group/start time is observed.
        # Its caller still owns wait/status; case cleanup adopts any descendants.
        if process.poll() is None:
            raise
    return process


def docker_labels():
    return ['--label', LABEL + '=' + os.environ['TALOS_E2E_RUN_ID']]


def socket_directory():
    """Unix socket paths must stay short even in a deeply nested worktree."""
    directory = Path(tempfile.mkdtemp(prefix='te2e-' + os.environ['TALOS_E2E_RUN_ID'] + '-', dir='/tmp'))
    (directory / '.e2e-owner').write_text(os.environ['TALOS_E2E_RUN_ID'])
    register('directory', directory)
    return directory


def remove_directory(record):
    directory = Path(record['value'])
    if not directory.exists():
        return
    if directory.is_symlink() or (directory / '.e2e-owner').read_text() != os.environ['TALOS_E2E_RUN_ID']:
        raise RuntimeError(f'Refusing directory owned by another run: {directory}')
    shutil.rmtree(directory)


def container_args(name):
    register('container', name)
    return ['--name', name, *docker_labels()]


def docker_create(kind, name, args, scope=None):
    register(kind, name, scope)
    command = ['docker', 'run', *docker_labels(), '--name', name, *args] if kind == 'container' else ['docker', kind, 'create', *docker_labels(), *args, name]
    try:
        result = subprocess.run(command, check=True, capture_output=True, text=True)
        print(result.stderr, file=sys.stderr, end='')
        return result.stdout.strip()
    except subprocess.CalledProcessError as error:
        print(error.stdout, file=sys.stderr)
        print(error.stderr, file=sys.stderr)
        raise


def stop_group(record):
    pid = int(record['value'])
    current = subprocess.run(['ps', '-p', str(pid), '-o', 'lstart='], capture_output=True, text=True)
    if current.returncode == 0 and current.stdout.strip() != record['started']:
        raise RuntimeError(f'Refusing reused process ID {pid}')
    def alive():
        rows = subprocess.check_output(['ps', '-axo', 'pid=,pgid=,stat='], text=True).splitlines()
        for row in rows:
            values = row.split()
            if len(values) == 3 and not values[2].startswith('Z'):
                if int(values[1 if record.get('group', True) else 0]) == pid:
                    return True
        return False
    # macOS returns EPERM for killpg(..., 0) on a zombie-only group. Observe
    # live members instead; never suppress a permission error on a live group.
    for sig, seconds in [(signal.SIGTERM, 3), (signal.SIGKILL, 2)]:
        if not alive():
            return
        try:
            (os.killpg if record.get('group', True) else os.kill)(pid, sig)
        except ProcessLookupError:
            return
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if not alive():
                return
            time.sleep(.05)
    if alive():
        raise RuntimeError(f'Process {pid} did not stop')


def remove_docker(record):
    kind, value = record['kind'], record['value']
    found = subprocess.run(['docker', kind, 'inspect', value], capture_output=True, text=True, timeout=15)
    if found.returncode:
        # A daemon failure is not proof that an owned resource has disappeared.
        subprocess.run(['docker', 'info'], check=True, capture_output=True, timeout=15)
        if 'no such' in found.stderr.lower() or 'not found' in found.stderr.lower():
            return
        raise RuntimeError(found.stderr.strip())
    doc = json.loads(found.stdout)[0]
    labels = doc.get('Config', {}).get('Labels') if kind in ('container', 'image') else doc.get('Labels')
    if (labels or {}).get(LABEL) != os.environ['TALOS_E2E_RUN_ID']:
        raise RuntimeError(f'Refusing resource owned by another run: {kind} {value}')
    command = ['docker', 'rm', '-f', value] if kind == 'container' else ['docker', kind, 'rm', value]
    subprocess.run(command, check=True, capture_output=True, timeout=30)


def cleanup(scope=None):
    ledger = root() / 'resources.jsonl'
    if not ledger.exists():
        return []
    records = [json.loads(line) for line in ledger.read_text().splitlines()]
    completed = []
    errors = []
    # Keep the ledger until the run succeeds; final cleanup can retry failures.
    for kind in ['process', 'container', 'volume', 'network', 'image', 'directory']:
        seen = set()
        for record in reversed(records):
            if record['kind'] != kind or (scope is not None and record['scope'] != scope):
                continue
            if record['value'] in seen:
                continue
            seen.add(record['value'])
            if kind == 'directory' and errors:
                continue
            try:
                if kind == 'process':
                    stop_group(record)
                elif kind == 'directory':
                    remove_directory(record)
                else:
                    remove_docker(record)
                completed.append(record)
            except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
                errors.append(f'Cleanup failed for {kind} {record["value"]}: {error}')
    with ledger.open('r+', encoding='utf-8') as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)
        remaining = [line for line in stream.read().splitlines() if json.loads(line) not in completed]
        stream.seek(0)
        stream.write(''.join(line + '\n' for line in remaining))
        stream.truncate()
    return errors


def adopt_case_processes(directory):
    """Backstop for product/MCP children that detach and outlive their parent."""
    prefixes = {str(directory) + '/', str(directory.resolve()) + '/'}
    directory = directory.resolve()
    if sys.platform == 'linux':
        paths = []
        for proc in Path('/proc').glob('[0-9]*/cwd'):
            try:
                paths.append((int(proc.parent.name), str(proc.resolve(strict=True))))
            except (OSError, RuntimeError):
                continue
    else:
        result = subprocess.run(['lsof', '-a', '-d', 'cwd', '-Fpn'], text=True, capture_output=True)
        paths = []
        pid = None
        for line in result.stdout.splitlines():
            if line.startswith('p'):
                pid = int(line[1:])
            elif line.startswith('n') and pid:
                paths.append((pid, line[1:]))
    arguments = {}
    for line in subprocess.check_output(['ps', '-axo', 'pid=,command='], text=True).splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) == 2 and any(prefix in parts[1] for prefix in prefixes):
            arguments[int(parts[0])] = str(directory)
    paths.extend(arguments.items())
    ledger = root() / 'resources.jsonl'
    registered = {int(json.loads(line)['value']) for line in ledger.read_text().splitlines() if json.loads(line)['kind'] == 'process'} if ledger.exists() else set()
    for pid, cwd in paths:
        if pid == os.getpid() or pid in registered or not Path(cwd).is_relative_to(directory):
            continue
        try:
            started = subprocess.check_output(['ps', '-p', str(pid), '-o', 'lstart='], text=True).strip()
        except subprocess.CalledProcessError:
            continue
        record = dict(kind='process', value=str(pid), started=started, group=False, scope=os.environ['TALOS_E2E_CASE_ID'])
        with (root() / 'resources.jsonl').open('a') as stream:
            fcntl.flock(stream, fcntl.LOCK_EX)
            stream.write(json.dumps(record) + '\n')
        registered.add(pid)


def build_image(base, supplied, args):
    if supplied:
        subprocess.run(['docker', 'image', 'inspect', supplied], check=True, stdout=subprocess.DEVNULL)
        return supplied
    name = base + ':e2e-' + os.environ['TALOS_E2E_RUN_ID']
    exists = subprocess.run(['docker', 'image', 'inspect', name], capture_output=True)
    if exists.returncode == 0:
        doc = json.loads(exists.stdout)[0]
        if (doc.get('Config', {}).get('Labels') or {}).get(LABEL) != os.environ['TALOS_E2E_RUN_ID']:
            raise RuntimeError('Image tag is not owned by this run')
        return name
    register('image', name, 'run')
    subprocess.run(['docker', 'build', *docker_labels(), '-t', name, *args], check=True, stdout=sys.stderr)
    return name


if __name__ == '__main__':
    action, *args = sys.argv[1:]
    if action == 'cleanup':
        errors = cleanup(args[0] if args else None)
        print('\n'.join(errors), file=sys.stderr)
        sys.exit(bool(errors))
    elif action == 'image':
        print(build_image(args[0], args[1], args[2:]))
    elif action == 'register':
        register(*args)
    elif action == 'docker':
        print(docker_create(args[0], args[1], args[2:]))
    else:
        raise SystemExit('Unknown lifecycle action')
