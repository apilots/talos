"""Turn additive ownership into named, dependency-complete executable CI tasks."""
from __future__ import annotations

import importlib.util
import json
from pathlib import Path

from ci_impact import CATEGORIES, ROOT, SERVER_TARGETS, frontend_graph, impact, rust_graph

BINARIES = {
    'server': ('talos-server', 'talos-server', 'TALOS_E2E_SERVER_BIN'),
    'cli': ('talos-tui', 'talos', 'TALOS_E2E_CLI_BIN'),
    'worker': ('talos-worker-bin', 'talos-worker', 'TALOS_E2E_WORKER_BIN'),
    'app_server': ('app-server', 'app-server', 'TALOS_E2E_APP_SERVER_BIN'),
    'test_client': ('talos-app-server-test-client', 'talos-app-server-test-client', 'TALOS_E2E_APP_SERVER_TEST_CLIENT_BIN'),
    'sandbox': ('talos-linux-sandbox', 'talos-linux-sandbox-launcher', 'TALOS_E2E_SANDBOX_BIN'),
}


def scenario_task(category: str, target: str, root: Path = ROOT) -> dict:
    spec = importlib.util.spec_from_file_location('ci_runner', root / 'talos-e2e/scripts/runner.py')
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    cases, requirements = [], {key: set() for key in ('commands', 'checks', 'binaries')}
    scenario_root = root / 'talos-e2e/scenarios'
    for item in target.splitlines():
        kind, value = item.split(':', 1)
        if kind not in ('case', 'group'):
            raise ValueError(f'Invalid scenario target: {item}')
        cases.extend(runner.select(scenario_root, **{kind: value}))
    for case in cases:
        for key, values in requirements.items():
            values.update(runner.requirements(scenario_root, case).get(key, []))
    docker = 'worker_image' in requirements['checks']
    binaries = requirements['binaries'] | ({'worker'} if docker else set())
    if binaries:
        binaries.add('sandbox')
    node = bool(requirements['commands'] & {'node', 'pnpm', 'npx'})
    return dict(name=target.splitlines()[0].split(':', 1)[1].replace('/', ' · '),
                kind='e2e', category=category, targets=target, cases=sorted(set(cases)),
                binaries=sorted(binaries), binary_pattern='ci-binary-' + ('{' + ','.join(sorted(binaries)) + '}' if len(binaries) > 1 else next(iter(binaries), 'none')), node=node, chrome='chrome' in requirements['checks'],
                docker=docker, rust_tests='cargo-nextest' in requirements['commands'],
                rust=['talos-tools'] if 'cargo-nextest' in requirements['commands'] else [], frontend=[])


def family(path: str) -> str:
    for prefix, name in (('talos-rs/apps/sdk', 'sdk'),
                         ('talos-rs/crates/orchestration/agent-client', 'client'),
                         ('talos-rs/crates/action/skills', 'skills'),
                         ('talos-rs/crates/action/mcp', 'mcp'),
                         ('talos-rs/crates/action/tools', 'tools'),
                         ('talos-rs/crates/action/', 'actions'),
                         ('talos-rs/bin/', 'server')):
        if path.startswith(prefix):
            return name
    return 'runtime'


def plan(paths: list[str], root: Path = ROOT) -> dict:
    selected, rust = impact(paths, root), rust_graph(root)
    categories = {c: {'quality': [], 'scenarios': {'include': []}} for c in CATEGORIES}
    for category, packages, ecosystem in (
        ('webui', selected.frontend - {'talos-desktop'}, 'frontend'),
        ('desktop', selected.frontend & {'talos-desktop'}, 'frontend'),
        ('tui', selected.rust & {'talos-tui'}, 'rust'),
        ('server', selected.rust - {'talos-tui'}, 'rust'),
    ):
        if packages:
            modules = sorted({family(rust.roots[p]) for p in packages}) if ecosystem == 'rust' else sorted(packages)
            categories[category]['quality'].append(dict(
                name=' / '.join(modules), kind='quality', category=category, ecosystem=ecosystem,
                rust=sorted(packages) if ecosystem == 'rust' else [],
                frontend=sorted(packages) if ecosystem == 'frontend' else [],
                binaries=[], node=ecosystem == 'frontend', chrome=False, docker=False))
    # A shared product scenario has one execution owner even when both its UI
    # and backend change. Unioning categories must not duplicate the same case.
    registration = SERVER_TARGETS['registration']
    if registration in selected.targets['server']:
        selected.targets['server'].remove(registration)
        selected.targets['webui'].add(registration)
    for category, targets in selected.targets.items():
        categories[category]['scenarios']['include'] = [scenario_task(category, target, root) for target in sorted(targets)]
    tests = {category: c['quality'][0]['rust'] for category, c in categories.items()
             if c['quality'] and c['quality'][0]['rust']}
    e2e_tests = sorted({p for c in categories.values() for t in c['scenarios']['include'] for p in t['rust']})
    if e2e_tests:
        tests['e2e'] = e2e_tests
    binaries = sorted({b for c in categories.values() for task in c['scenarios']['include'] for b in task['binaries']}
                      | ({'sandbox'} if tests else set()))
    return dict(categories=categories, installer=selected.installer, mcp=selected.mcp,
                harness=selected.harness,
                build=dict(name='Build · shared debug binaries', kind='build', category='build',
                           binaries=binaries, rust=[], tests={}, frontend=[], node=False, chrome=False, docker=False),
                test_build=dict(name='Build · shared Rust test archives', kind='build', category='test-build',
                                binaries=[], rust=sorted({p for group in tests.values() for p in group}),
                                tests=tests, frontend=[], node=False, chrome=False, docker=False))


def task_roots(task: dict, root: Path = ROOT) -> set[str]:
    rust, frontend = rust_graph(root), frontend_graph(root)
    rust_names = set(task.get('rust', [])) | {BINARIES[b][0] for b in task.get('binaries', [])}
    frontend_names = set(task.get('frontend', []))
    if task.get('node') and task['kind'] == 'e2e':
        frontend_names.update(frontend.roots)
    roots = {rust.roots[n] for n in rust.closure(rust_names)}
    roots.update(frontend.roots[n] for n in frontend.closure(frontend_names))
    roots.update({'.github/workflows', '.github/actions', '.github/scripts', '.github/release-toolchain.env', '.github/ci-toolchain.env', '.github/sandbox-base.env',
                  'scripts', 'conf', 'justfile', 'build', 'build.sh', 'resources'})
    if rust_names:
        roots.update({'talos-rs/Cargo.toml', 'talos-rs/Cargo.lock', 'talos-rs/.cargo',
                      '.cargo', 'rust-toolchain.toml', 'rust-toolchain'})
        # Every manifest participates in resolution, even outside a source closure.
        roots.update(path + '/Cargo.toml' for path in rust.roots.values())
    if frontend_names:
        roots.update({'talos-app/package.json', 'talos-app/pnpm-lock.yaml', 'talos-app/pnpm-workspace.yaml',
                      'talos-app/scripts', 'talos-app/shared'})
        roots.update(p.relative_to(root).as_posix() for p in (root / 'talos-app').glob('*') if p.is_file())
    if task.get('tests'):
        roots.add('talos-rs')  # Test fixtures may reference other workspace members.
    if task['kind'] == 'harness':
        roots.update({'talos-e2e', 'talos-rs', 'talos-app'})
    if task['kind'] == 'e2e':
        roots.update({'talos-e2e/run_e2e.sh', 'talos-e2e/scripts', 'talos-e2e/fixtures', 'talos-e2e/docker'})
        roots.update('talos-e2e/scenarios/' + case for case in task['cases'])
        roots.update('talos-e2e/scenarios/' + str(Path(case).parent / 'requirements.json') for case in task['cases'])
    if task['kind'] == 'mcp':
        roots.update({'talos-rs', '.cargo', 'rust-toolchain.toml', 'rust-toolchain',
                      'talos-e2e', 'deploy/install'})
    return roots
