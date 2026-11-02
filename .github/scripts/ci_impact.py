"""Package ownership and additive hosted acceptance selection (no network/tools required)."""
from __future__ import annotations

from dataclasses import dataclass, field
import importlib.util
import json
from pathlib import Path
import tomllib

ROOT = Path(__file__).resolve().parents[2]
CATEGORIES = ('webui', 'desktop', 'tui', 'server')
SERVER_TARGETS = {
    'models': 'group:01_models',
    'app-server': 'case:09_app_server/smoke_e2e\ncase:09_app_server/desktop_recovery',
    'registration': 'case:09_app_server/computer_deletion',
    'files': 'group:12_files', 'sandbox': 'group:14_sandbox',
    'output-logs': 'case:06_tools/output_logs',
}


def scenario_runner(root: Path = ROOT):
    spec = importlib.util.spec_from_file_location('ci_runner', root / 'talos-e2e/scripts/runner.py')
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    return runner


def client_group_targets(group: str, root: Path = ROOT) -> dict[str, set[str]]:
    """Use the existing catalog and prerequisites, not case names, for client groups."""
    runner = scenario_runner(root)
    scenario_root = root / 'talos-e2e/scenarios'
    targets = {category: set() for category in CATEGORIES}
    for case in runner.select(scenario_root, group=group):
        checks = set(runner.requirements(scenario_root, case).get('checks', []))
        category = ('server' if 'server_image' in checks else
                    'desktop' if checks & {'display', 'worker_image'} else 'webui')
        targets[category].add('case:' + case.removesuffix('.sh'))
    return targets


WORKSPACE_CASES = client_group_targets('22_workspace_resources')
PREVIEW_CASES = client_group_targets('21_resource_preview')
NATIVE_BROWSER_CASES = client_group_targets('20_browser_workspace')['desktop']
CONTAINER_CASES = client_group_targets('23_browser_container')
BROWSER_CASES = (WORKSPACE_CASES['webui'] | WORKSPACE_CASES['desktop']
                 | NATIVE_BROWSER_CASES | CONTAINER_CASES['desktop'])
TUI_TARGET = 'case:08_tui/controlled_chat'
MEDIA_OWNERS = ('desktop-media', 'desktop-ipc', 'desktop-server-requests', 'ipc-guard',
                'ipc.', 'preload.', 'app-server-controller')


def is_document(path: str) -> bool:
    if not path.lower().endswith(('.md', '.mdx', '.rst', '.adoc')):
        return False
    if not path.startswith(('docs/', 'spec/')) and any(
            part in Path(path).parts for part in ('resources', 'templates', 'fixtures', 'prompts')):
        return False
    name = Path(path).name.lower()
    return (path.startswith(('docs/', 'spec/', '.github/')) or '/' not in path
            or name.startswith(('readme.', 'readme_', 'changelog.', 'contributing.', 'contributing_'))
            or name in ('agents.md', 'lifecycle.md', 'implementation_plan.md'))


@dataclass
class PackageGraph:
    roots: dict[str, str]
    dependencies: dict[str, set[str]]

    def closure(self, names: set[str], *, reverse: bool = False) -> set[str]:
        edges = self.dependencies
        if reverse:
            edges = {name: {n for n, deps in edges.items() if name in deps} for name in self.roots}
        result, pending = set(names), list(names)
        while pending:
            for name in edges.get(pending.pop(), set()) - result:
                result.add(name)
                pending.append(name)
        return result

    def owner(self, path: str) -> str | None:
        return next((n for n, root in sorted(self.roots.items(), key=lambda p: -len(p[1]))
                     if path.startswith(root + '/')), None)


def rust_graph(root: Path = ROOT) -> PackageGraph:
    workspace = tomllib.loads((root / 'talos-rs/Cargo.toml').read_text())
    roots, dependencies = {}, {}
    for member in workspace['workspace']['members']:
        path = root / 'talos-rs' / member / 'Cargo.toml'
        # A deleted/moved member must fail planning until its manifest is consistent.
        data = tomllib.loads(path.read_text())
        name = data['package']['name']
        roots[name] = f'talos-rs/{member}'
        deps = set()
        for table in [data, *data.get('target', {}).values()]:
            for key in ('dependencies', 'dev-dependencies', 'build-dependencies'):
                for alias, dep in table.get(key, {}).items():
                    if isinstance(dep, dict) and dep.get('workspace'):
                        dep = workspace.get('workspace', {}).get('dependencies', {}).get(alias)
                    if isinstance(dep, dict) and 'path' in dep:
                        deps.add(dep.get('package', alias))
        dependencies[name] = deps
    return PackageGraph(roots, dependencies)


def frontend_graph(root: Path = ROOT) -> PackageGraph:
    manifests = [root / 'talos-app/web/package.json', root / 'talos-app/desktop/package.json',
                 *sorted((root / 'talos-app/ts-sdk/packages').glob('*/package.json'))]
    roots, dependencies = {}, {}
    for path in manifests:
        data = json.loads(path.read_text())
        name = data['name']
        roots[name] = path.parent.relative_to(root).as_posix()
        dependencies[name] = {n for key in ('dependencies', 'devDependencies', 'peerDependencies')
                              for n, version in data.get(key, {}).items() if version.startswith('workspace:')}
    return PackageGraph(roots, dependencies)


@dataclass
class Impact:
    rust: set[str] = field(default_factory=set)
    frontend: set[str] = field(default_factory=set)
    targets: dict[str, set[str]] = field(default_factory=lambda: {c: set() for c in CATEGORIES})
    installer: bool = False
    mcp: bool = False
    harness: bool = False

    def full(self, rust: PackageGraph, frontend: PackageGraph, *, frontend_checks: bool = True) -> None:
        self.rust.update(rust.roots)
        if frontend_checks:
            self.frontend.update(frontend.roots)
        self.targets['server'].update(SERVER_TARGETS.values())
        self.targets['server'].update(CONTAINER_CASES['server'])
        self.targets['desktop'].update(BROWSER_CASES - WORKSPACE_CASES['webui'])
        self.targets['webui'].update(WORKSPACE_CASES['webui'])
        for category, cases in PREVIEW_CASES.items():
            self.targets[category].update(cases)
        self.targets['tui'].add(TUI_TARGET)
        self.installer = self.mcp = self.harness = True


def impact(paths: list[str], root: Path = ROOT) -> Impact:
    rust, frontend = rust_graph(root), frontend_graph(root)
    result = Impact()
    direct_rust, direct_frontend = set(), set()
    for path in paths:
        if is_document(path):
            continue
        if path.startswith('talos-rs/'):
            owner = rust.owner(path)
            if owner:
                direct_rust.add(owner)
                if path.startswith('talos-rs/apps/tui/src/cli/'):
                    result.targets['tui'].add('case:05_cli/plugin_lifecycle')
                if path == 'talos-rs/apps/tui/src/input.rs':
                    result.targets['server'].update(CONTAINER_CASES['server'])

            else:
                result.full(rust, frontend, frontend_checks=False)
        elif path.startswith('talos-app/'):
            owner = frontend.owner(path)
            if owner:
                direct_frontend.add(owner)
                if owner == 'talos-desktop':
                    result.targets['desktop'].update(NATIVE_BROWSER_CASES | WORKSPACE_CASES['desktop'])
                    if not path.startswith(('talos-app/desktop/src/', 'talos-app/desktop/resources/')) or any(
                            path.startswith('talos-app/desktop/src/' + name) for name in MEDIA_OWNERS):
                        result.targets['desktop'].update(CONTAINER_CASES['desktop'])
                elif owner == 'talos-web':
                    result.targets['webui'].update({SERVER_TARGETS['registration']} | WORKSPACE_CASES['webui'])
                    # The shared renderer is shipped inside the native Desktop host.
                    result.targets['desktop'].update(WORKSPACE_CASES['desktop'])
                    if path.startswith('talos-app/web/src/features/computers/ComputerDesktop'):
                        result.targets['desktop'].update(CONTAINER_CASES['desktop'])
                else:
                    result.targets['webui'].update({SERVER_TARGETS['registration']} | WORKSPACE_CASES['webui'])
                    result.targets['desktop'].update(BROWSER_CASES - WORKSPACE_CASES['webui'])
            elif path.startswith('talos-app/shared/'):
                direct_frontend.update({'talos-web', 'talos-desktop'})
                result.targets['webui'].update(WORKSPACE_CASES['webui'])
                result.targets['desktop'].update(BROWSER_CASES - WORKSPACE_CASES['webui'])
            else:
                # Frontend lockfiles, workspace config, generators and unknown inputs
                # expand within their ecosystem, never to unrelated Rust test suites.
                direct_frontend.update(frontend.roots)
                result.targets['webui'].update({SERVER_TARGETS['registration']} | WORKSPACE_CASES['webui'])
                result.targets['desktop'].update(BROWSER_CASES - WORKSPACE_CASES['webui'])
        elif path.startswith(('deploy/install/', '.github/workflows/release-install',
                              '.github/workflows/release-preflight')) or path == '.github/workflows/release.yml':
            result.installer = True
        elif path == 'build.sh' or path.startswith('build/scripts/'):
            result.full(rust, frontend)
        elif path.startswith('build/worker/'):
            result.targets['desktop'].update(CONTAINER_CASES['desktop'])
            if path.startswith('build/worker/browser/'):
                result.targets['server'].update(CONTAINER_CASES['server'])
        elif path.startswith('build/server/'):
            result.targets['server'].update(SERVER_TARGETS.values())
            result.targets['server'].update(CONTAINER_CASES['server'])
        elif path.startswith('build/container/'):
            result.targets['desktop'].update(CONTAINER_CASES['desktop'])
            result.targets['server'].update(CONTAINER_CASES['server'])
        elif path.startswith('build/'):
            result.full(rust, frontend)
        elif path.startswith('talos-e2e/'):
            result.harness = True
            if path.startswith(('talos-e2e/scenarios/21_resource_preview/', 'talos-e2e/scripts/resource_preview/', 'talos-e2e/fixtures/resource-preview/')):
                for category, cases in client_group_targets('21_resource_preview', root).items():
                    result.targets[category].update(cases)
            elif path.startswith('talos-e2e/scenarios/22_workspace_resources/'):
                for category, cases in client_group_targets('22_workspace_resources', root).items():
                    result.targets[category].update(cases)
            elif path.startswith('talos-e2e/scripts/workspace_resources/'):
                # The format-preview group reuses this Web/Desktop workspace harness.
                for group in ('22_workspace_resources', '21_resource_preview'):
                    for category, cases in client_group_targets(group, root).items():
                        result.targets[category].update(cases)
            elif path.startswith('talos-e2e/scenarios/20_browser_workspace/'):
                result.targets['desktop'].update(NATIVE_BROWSER_CASES)
            elif path.startswith('talos-e2e/scenarios/23_browser_container/'):
                for category, cases in CONTAINER_CASES.items():
                    result.targets[category].update(cases)
            elif path in ('talos-e2e/scripts/plugin_cli.py',
                        'talos-e2e/scenarios/05_cli/plugin_lifecycle.sh',
                        'talos-e2e/scenarios/05_cli/requirements.json'):
                result.targets['tui'].add('case:05_cli/plugin_lifecycle')
                if path == 'talos-e2e/scripts/plugin_cli.py':
                    result.targets['desktop'].update(CONTAINER_CASES['desktop'])
            elif path.startswith(('talos-e2e/scenarios/15_tui_install/', 'talos-e2e/scripts/tui_install.sh',
                                'talos-e2e/tests/tui_install_target_test.sh', 'talos-e2e/docker/tui-install-')):
                result.installer = True
            elif path.startswith(('talos-e2e/scenarios/08_tui/', 'talos-e2e/scripts/tui')):
                result.targets['tui'].add(TUI_TARGET)
            elif path == 'talos-e2e/scripts/computer_workspace/deletion.py':
                result.targets['webui'].add(SERVER_TARGETS['registration'])
            elif path.startswith('talos-e2e/scripts/browser-worker/'):
                result.targets['desktop'].update(NATIVE_BROWSER_CASES | CONTAINER_CASES['desktop'])
                result.targets['server'].update(CONTAINER_CASES['server'])
            elif path.startswith('talos-e2e/scenarios/') and any(
                    path.startswith('talos-e2e/scenarios/' + prefix) for prefix in
                    ('01_models/', '09_app_server/', '12_files/', '14_sandbox/', '06_tools/')):
                group = path.split('/')[2]
                key = {'01_models': 'models', '09_app_server': 'app-server',
                       '12_files': 'files', '14_sandbox': 'sandbox', '06_tools': 'output-logs'}[group]
                result.targets['server'].add(SERVER_TARGETS[key])
                if path.endswith('computer_deletion.sh'):
                    result.targets['webui'].add('case:09_app_server/computer_deletion')
            else:
                # Shared runner/fixtures and provider-backed scenario edits retain the
                # public representative suites instead of silently skipping behavior.
                result.full(rust, frontend, frontend_checks=False)
        elif path.startswith(('conf/', 'talos-rs/.cargo/')) or path in ('justfile', '.github/release-toolchain.env', '.github/ci-toolchain.env', '.github/sandbox-base.env'):
            result.full(rust, frontend, frontend_checks=path == '.github/release-toolchain.env')
        elif path.startswith(('.github/scripts/ci_', '.github/scripts/pr_change_scope',
                              '.github/actions/ci-', '.github/workflows/ci-',
                              '.github/workflows/talos-', '.github/workflows/pr-e2e')):
            result.full(rust, frontend)
        elif path.startswith('scripts/check_error_contract'):
            result.rust.update(rust.roots)
            result.frontend.update(frontend.roots)
        elif path.startswith(('docs/', 'spec/', '.github/', 'deploy/', '.githooks/')) or path in (
                'LICENSE', 'NOTICE', '.gitignore', '.gitattributes'):
            continue
        else:
            # Unclassified product/build inputs must never become an implicit skip.
            result.full(rust, frontend)
    affected = rust.closure(direct_rust, reverse=True)
    result.rust.update(affected)
    if direct_rust & {'talos-types', 'talos-app-protocol', 'agent-protocol'}:
        # Generated wire contracts have both Rust and TypeScript consumers.
        direct_frontend.update(frontend.roots)
    result.frontend.update(frontend.closure(direct_frontend, reverse=True))
    if 'talos-web' in result.frontend:
        result.targets['webui'].update(PREVIEW_CASES['webui'])
        result.targets['desktop'].update(PREVIEW_CASES['desktop'])
    elif 'talos-desktop' in result.frontend:
        result.targets['desktop'].update(PREVIEW_CASES['desktop'])
    if any(path.startswith(('talos-rs/crates/orchestration/service/src/file/',
                            'talos-rs/crates/orchestration/service/src/fs/preview',
                            'talos-rs/crates/orchestration/service/src/service/file_preview',
                            'talos-rs/crates/common/fs/src/rooted',
                            'talos-rs/crates/gateway/http/src/server/file_preview',
                            'talos-rs/crates/orchestration/agent-client/src/environment',
                            'talos-rs/crates/orchestration/agent-protocol/src/environment',
                            'talos-rs/crates/orchestration/agent-pb/proto/environment',
                            'talos-rs/crates/infra/config/src/domains/storage')) for path in paths):
        for category, cases in PREVIEW_CASES.items():
            result.targets[category].update(cases)
    # Expand each owner's acceptance independently, then union the results. A
    # TUI+MCP edit must not accidentally take a broader route than those edits alone.
    for owner in direct_rust:
        consumers = rust.closure({owner}, reverse=True)
        if 'talos-tui' in consumers:
            result.targets['tui'].add(TUI_TARGET)
        focused = rust.roots[owner].startswith((
            'talos-rs/crates/action/', 'talos-rs/apps/sdk',
            'talos-rs/crates/orchestration/agent-client'))
        if focused:
            result.targets['server'].add(SERVER_TARGETS['app-server'])
            if owner in {'talos-tools', 'talos-tools-policy', 'talos-tools-sandbox',
                         'talos-linux-sandbox', 'talos-macos-sandbox', 'talos-browser'}:
                result.targets['server'].update(SERVER_TARGETS[k] for k in ('models', 'output-logs', 'sandbox', 'files'))
            if owner in {'talos-skills', 'talos-plugin'}:
                result.targets['server'].add(SERVER_TARGETS['models'])
            if owner in {'talos-plugin', 'talos-mcp', 'talos-browser'}:
                result.targets['server'].update(CONTAINER_CASES['server'])
            if owner == 'talos-mcp':
                result.targets['desktop'].update(NATIVE_BROWSER_CASES)
            if owner == 'talos-browser':
                result.targets['desktop'].update(BROWSER_CASES - WORKSPACE_CASES['webui'])
        elif consumers & {'talos-server', 'talos-worker-bin'}:
            result.targets['server'].update(SERVER_TARGETS.values())
            result.targets['server'].update(CONTAINER_CASES['server'])
            result.targets['webui'].update(WORKSPACE_CASES['webui'])
            result.targets['desktop'].update(BROWSER_CASES - WORKSPACE_CASES['webui'])
        elif consumers & {'app-server', 'talos-app-server-test-client'}:
            result.targets['server'].add(SERVER_TARGETS['app-server'])
            result.targets['desktop'].update(BROWSER_CASES - WORKSPACE_CASES['webui'])
    result.mcp |= 'talos-mcp' in affected
    return result
