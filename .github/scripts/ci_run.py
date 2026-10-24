"""Execute declared CI tasks through repository commands and exact-input records."""
from __future__ import annotations

import datetime
import hashlib
import json
import os
import re
from pathlib import Path
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

from ci_artifacts import inventory, pack_binary, seal, verified
from ci_impact import ROOT, is_document
from ci_plan import BINARIES, task_roots


def run(*command: str, env: dict | None = None) -> None:
    print('+ ' + ' '.join(command), flush=True)
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def fingerprint(task: dict, root: Path = ROOT) -> str:
    roots = task_roots(task, root)
    if task['kind'] == 'quality':
        # fmt-check and check:repo are repository-wide contracts. Their complete
        # source inputs participate in reuse even when executable tests are scoped.
        roots.add('talos-rs')
        if task['frontend']:
            roots.add('talos-app')
    paths = subprocess.check_output(['git', 'ls-files', '--cached', '--others', '--exclude-standard', '-z'], cwd=root).decode().split('\0')
    digest = hashlib.sha256(json.dumps(task, sort_keys=True).encode())
    for path in sorted(set(paths) - {''}):
        if is_document(path):
            continue
        if not any(path == r or path.startswith(r + '/') for r in roots):
            continue
        file = root / path
        if not file.exists() and not file.is_symlink():
            continue
        content = os.readlink(file).encode() if file.is_symlink() else file.read_bytes()
        # Git tracks file kind and the executable bit, not each runner's umask.
        digest.update(json.dumps([path, file.is_symlink(), file.lstat().st_mode & 0o100, len(content)]).encode())
        digest.update(content)
    return digest.hexdigest()


def cache_key(task: dict) -> tuple[str, Path]:
    environment = [os.environ.get(n, '') for n in ('RUNNER_OS', 'RUNNER_ARCH', 'ImageOS', 'ImageVersion')]
    environment.append(json.dumps({name: value for name, value in os.environ.items()
        if name.startswith(('RUSTFLAGS', 'CARGO_PROFILE_', 'CFLAGS', 'CXXFLAGS', 'CC_', 'CXX_'))}, sort_keys=True))
    environment.append(datetime.datetime.now(datetime.timezone.utc).date().isoformat())
    digest = hashlib.sha256((fingerprint(task) + json.dumps(environment)).encode()).hexdigest()
    # GitHub additionally scopes caches to the branch/PR merge ref. This key also
    # prevents a base-branch success record from becoming a cross-PR test result.
    scope = os.environ.get('CI_PR_NUMBER', 'local')
    key = f'talos-ci-v1-{scope}-{task["kind"]}-{digest}'
    return key, Path(os.environ.get('RUNNER_TEMP', str(ROOT / '.tmp'))) / 'ci-results' / key


def emit(**values: str) -> None:
    output = os.environ.get('GITHUB_OUTPUT')
    if output:
        with open(output, 'a') as stream:
            for name, value in values.items():
                stream.write(f'{name}={value}\n')
    else:
        print(json.dumps(values))


def cache_storage(task: dict, key: str) -> tuple[str, str]:
    """Keep immutable build blobs; let each check run publish a fresh record."""
    run, attempt = os.environ.get('GITHUB_RUN_ID', ''), os.environ.get('GITHUB_RUN_ATTEMPT', '')
    if task['kind'] == 'build' or not run.isdigit() or not attempt.isdigit():
        return key, ''
    storage = f'{key}-run-{run}-{attempt}'
    token, repository, ref = (os.environ.get(name) for name in ('GH_TOKEN', 'GITHUB_REPOSITORY', 'GITHUB_REF'))
    if not token or not repository or not ref:
        return storage, ''
    query = urllib.parse.urlencode(dict(key=key + '-run-', ref=ref, per_page=100,
                                       sort='created_at', direction='desc'))
    request = urllib.request.Request(os.environ.get('GITHUB_API_URL', 'https://api.github.com') +
        f'/repos/{repository}/actions/caches?{query}',
        headers={'Authorization': f'Bearer {token}', 'Accept': 'application/vnd.github+json'})
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            rows = json.load(response).get('actions_caches', [])
        for row in rows:
            name = row.get('key', '')
            if row.get('ref') == ref and re.fullmatch(re.escape(key) + r'-run-\d+-\d+', name) and name != storage:
                # Resolve one complete storage key, never a partial-input restore.
                # Payload and successful origin are still checked after restoration.
                return storage, name
    except (OSError, ValueError, TypeError, AttributeError):
        pass
    return storage, ''


def cached(directory: Path, key: str, task: dict) -> bool:
    try:
        record = json.loads((directory / 'success.json').read_text())
        if record['key'] != key or record['conclusion'] != 'success' or record['task'] != task:
            return False
        if task['kind'] == 'build':
            return bool(record['artifacts']) and inventory(directory) == record['artifacts']
        return True
    except (OSError, ValueError, KeyError):
        return False


def reusable(directory: Path, key: str, task: dict) -> bool:
    if not cached(directory, key, task):
        return False
    if task['kind'] == 'build':
        return True
    if os.environ.get('CI_FORCE_RECHECK') == 'true':
        return False
    if os.environ.get('GITHUB_ACTIONS') != 'true':
        return True
    record = json.loads((directory / 'success.json').read_text())
    token, repository = os.environ.get('GH_TOKEN'), os.environ.get('GITHUB_REPOSITORY')
    if not token or not repository or not record.get('run'):
        return False
    # A previous successful task from a cancelled/failed workflow is not evidence.
    # The run endpoint reports the latest rerun attempt, so a later failed retry
    # also invalidates an older immutable success cache with the same input key.
    request = urllib.request.Request(
        os.environ.get('GITHUB_API_URL', 'https://api.github.com') +
        f'/repos/{repository}/actions/runs/{record["run"]}',
        headers={'Authorization': f'Bearer {token}', 'Accept': 'application/vnd.github+json'})
    try:
        with urllib.request.urlopen(request, timeout=15) as response:
            run = json.load(response)
        return run.get('status') == 'completed' and run.get('conclusion') == 'success'
    except (OSError, ValueError, urllib.error.URLError):
        return False


def record(directory: Path, key: str, task: dict) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    value = dict(key=key, conclusion='success', task=task,
                 sha=os.environ.get('GITHUB_SHA'), run=os.environ.get('GITHUB_RUN_ID'),
                 attempt=os.environ.get('GITHUB_RUN_ATTEMPT'))
    if task['kind'] == 'build':
        value['artifacts'] = inventory(directory)
    (directory / 'success.json').write_text(json.dumps(value, indent=2))


def execute(task: dict, directory: Path) -> None:
    if task['kind'] == 'quality':
        if task['rust']:
            args = [value for package in task['rust'] for value in ('-p', package)]
            run('just', 'fmt-check')
            run('just', 'check', '--locked', *args)
            archive = os.environ.get('TALOS_TEST_ARCHIVE')
            if archive:
                verified(Path(archive))
                run('just', 'test')  # The archive contains exactly this task's packages.
            else:
                run('just', 'test', '--locked', *args)
        if task['frontend']:
            filters = [value for package in task['frontend'] for value in ('--filter', package)]
            for script in ('lint', 'test', 'typecheck'):
                run('pnpm', '--dir', 'talos-app', '-r', *filters, script)
            for package in task['frontend']:
                run('pnpm', '--dir', 'talos-app', '--filter', package,
                    'build:main' if package == 'talos-desktop' else 'build')
            run('pnpm', '--dir', 'talos-app', 'check:repo')
    elif task['kind'] == 'build':
        args = [value for binary in task['binaries'] for value in
                ('-p', BINARIES[binary][0], '--bin', BINARIES[binary][1])]
        if args:
            run('just', 'build', '--locked', *args)
        directory.mkdir(parents=True, exist_ok=True)
        for binary in task['binaries']:
            pack_binary(directory, binary, ROOT / 'talos-rs/target/debug' / BINARIES[binary][1])
        for owner, packages in task.get('tests', {}).items():
            archive = directory / f'tests-{owner}.tar.zst'
            package_args = [arg for package in packages for arg in ('-p', package)]
            run('just', 'test-archive', '--locked', *package_args, '--archive-file', str(archive))
            seal(archive)
    elif task['kind'] == 'e2e':
        env = dict(os.environ)
        if task.get('rust_tests'):
            env['TALOS_TEST_ARCHIVE'] = str(verified(ROOT / 'output/ci-tests/tests-e2e.tar.zst'))
            values = dict(line.split('=', 1) for line in (ROOT / '.github/sandbox-base.env').read_text().splitlines() if '=' in line)
            image = values['CI_SANDBOX_BASE_IMAGE']
            if not re.fullmatch(r'ghcr.io/[a-z0-9/._-]+@sha256:[0-9a-f]{64}', image):
                raise ValueError('Sandbox base image must be digest-pinned')
            run('docker', 'pull', image)
            env['SANDBOX_DOCKER_IMAGE'] = image
        for binary in task['binaries']:
            _, name, variable = BINARIES[binary]
            path = ROOT / 'output/ci-binaries' / name
            if not path.is_file() or not os.access(path, os.X_OK):
                raise ValueError(f'Missing executable from current-run artifact: {name}')
            env[variable] = str(path)
        if task['docker']:
            destination = ROOT / '.release-product/bin'
            destination.mkdir(parents=True, exist_ok=True)
            import shutil
            for name in ('talos-worker', 'talos-linux-sandbox-launcher'):
                shutil.copy2(ROOT / 'output/ci-binaries' / name, destination / name)
            run('docker', 'build', '--build-arg', 'PRODUCT_STAGE=packaged', '-f',
                'build/worker/Dockerfile', '-t', 'talos-worker:browser-e2e', '..')
            env['TALOS_E2E_WORKER_IMAGE'] = 'talos-worker:browser-e2e'
        for target in task['targets'].splitlines():
            kind, value = target.split(':', 1)
            if kind not in ('case', 'group'):
                raise ValueError(f'Invalid target: {target}')
            command = ('just', 'e2e-case' if kind == 'case' else 'e2e-group', value)
            run(*(('xvfb-run', '-a') + command if sys.platform == 'linux' else command), env=env)
    elif task['kind'] == 'harness':
        env = dict(os.environ)
        env['PYTHONPATH'] = os.pathsep.join(filter(None, (
            str(ROOT / 'talos-e2e/scripts'), env.get('PYTHONPATH'))))
        for directory in ('talos-e2e/tests', 'talos-e2e/scripts/computer_sync',
                          'talos-e2e/scripts/browser-worker'):
            run(sys.executable, '-m', 'unittest', 'discover', '-s', directory,
                '-p', '*_test.py', '-v', env=env)
    elif task['kind'] == 'mcp':
        run('just', 'test', '--locked', '-p', 'talos-mcp')
    else:
        raise ValueError(f'Unknown CI task: {task["kind"]}')


def main() -> None:
    task = json.loads(os.environ['CI_TASK'])
    action = sys.argv[1]
    key, default_directory = cache_key(task)
    key = os.environ.get('CI_CACHE_KEY', key)
    directory = Path(os.environ.get('CI_CACHE_DIR', str(default_directory)))
    if action == 'key':
        storage, history = cache_storage(task, key)
        emit(key=key, path=str(directory), storage_key=storage, restore_key=history)
    elif action == 'check':
        hit = reusable(directory, key, task)
        emit(hit=str(hit).lower())
        if hit:
            prior = json.loads((directory / 'success.json').read_text())
            summary = os.environ.get('GITHUB_STEP_SUMMARY')
            if summary:
                with open(summary, 'a') as stream:
                    stream.write(f'Exact-input reuse: `{task["name"]}`; successful run '
                                 f'`{prior["run"]}` / `{prior["sha"]}`; key `{key}`.\n')
    elif action == 'record':
        record(directory, key, task)
    elif action == 'run':
        execute(task, directory)
        # A failing command exits before writing the success record. Cache action
        # post-save also requires the entire job (including cleanup) to succeed.
        record(directory, key, task)
    else:
        raise ValueError(action)


if __name__ == '__main__':
    main()
