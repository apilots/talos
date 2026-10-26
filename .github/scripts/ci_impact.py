"""Package ownership and additive hosted acceptance selection (no network/tools required)."""
from __future__ import annotations

from dataclasses import dataclass, field
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
BROWSER_CASES = {f'case:20_browser_workspace/{name}' for name in (
    '01_resource_tabs', '02_desktop_resources', '03_desktop_browser', '04_desktop_mcp', '05_docker_worker')}
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
        self.targets['desktop'].update(BROWSER_CASES - {'case:20_browser_workspace/01_resource_tabs'})
        self.targets['webui'].add('case:20_browser_workspace/01_resource_tabs')
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

            else:
                result.full(rust, frontend, frontend_checks=False)
        elif path.startswith('talos-app/'):
            owner = frontend.owner(path)
            if owner:
                direct_frontend.add(owner)
                if owner == 'talos-desktop':
                    result.targets['desktop'].update(BROWSER_CASES - {
                        'case:20_browser_workspace/01_resource_tabs', 'case:20_browser_workspace/05_docker_worker'})
                    if not path.startswith(('talos-app/desktop/src/', 'talos-app/desktop/resources/')) or any(
                            path.startswith('talos-app/desktop/src/' + name) for name in MEDIA_OWNERS):
                        result.targets['desktop'].add('case:20_browser_workspace/05_docker_worker')
                elif owner == 'talos-web':
                    result.targets['webui'].update({'case:09_app_server/computer_deletion',
                                                   'case:20_browser_workspace/01_resource_tabs'})
                    # The shared renderer is shipped inside the native Desktop host.
                    result.targets['desktop'].add('case:20_browser_workspace/02_desktop_resources')
                    if path.startswith('talos-app/web/src/features/computers/ComputerDesktop'):
                        result.targets['desktop'].add('case:20_browser_workspace/05_docker_worker')
                else:
                    result.targets['webui'].update({'case:09_app_server/computer_deletion',
                                                   'case:20_browser_workspace/01_resource_tabs'})
                    result.targets['desktop'].update(BROWSER_CASES - {'case:20_browser_workspace/01_resource_tabs'})
            elif path.startswith('talos-app/shared/'):
                direct_frontend.update({'talos-web', 'talos-desktop'})
                result.targets['webui'].add('case:20_browser_workspace/01_resource_tabs')
                result.targets['desktop'].update(BROWSER_CASES - {'case:20_browser_workspace/01_resource_tabs'})
            else:
                # Frontend lockfiles, workspace config, generators and unknown inputs
                # expand within their ecosystem, never to unrelated Rust test suites.
                direct_frontend.update(frontend.roots)
                result.targets['webui'].update({'case:09_app_server/computer_deletion',
                                               'case:20_browser_workspace/01_resource_tabs'})
                result.targets['desktop'].update(BROWSER_CASES - {'case:20_browser_workspace/01_resource_tabs'})
        elif path.startswith(('deploy/install/', '.github/workflows/release-install',
                              '.github/workflows/release-preflight')) or path == '.github/workflows/release.yml':
            result.installer = True
        elif path == 'build.sh' or path.startswith('build/scripts/'):
            result.full(rust, frontend)
        elif path.startswith('build/worker/'):
            result.targets['desktop'].add('case:20_browser_workspace/05_docker_worker')
        elif path.startswith('build/server/'):
            result.targets['server'].update(SERVER_TARGETS.values())
        elif path.startswith('build/'):
            result.full(rust, frontend)
        elif path.startswith('talos-e2e/'):
            result.harness = True
            if path in ('talos-e2e/scripts/plugin_cli.py',
                        'talos-e2e/scenarios/05_cli/plugin_lifecycle.sh',
                        'talos-e2e/scenarios/05_cli/requirements.json'):
                result.targets['tui'].add('case:05_cli/plugin_lifecycle')
                if path == 'talos-e2e/scripts/plugin_cli.py':
                    result.targets['desktop'].add('case:20_browser_workspace/05_docker_worker')
            elif path.startswith(('talos-e2e/scenarios/15_tui_install/', 'talos-e2e/scripts/tui_install.sh',
                                'talos-e2e/tests/tui_install_target_test.sh', 'talos-e2e/docker/tui-install-')):
                result.installer = True
            elif path.startswith(('talos-e2e/scenarios/08_tui/', 'talos-e2e/scripts/tui')):
                result.targets['tui'].add(TUI_TARGET)
            elif path == 'talos-e2e/scripts/computer_workspace/resources.py':
                result.targets['webui'].add('case:20_browser_workspace/01_resource_tabs')
            elif path == 'talos-e2e/scripts/computer_workspace/deletion.py':
                result.targets['webui'].add('case:09_app_server/computer_deletion')
            elif path.startswith('talos-e2e/scripts/browser-worker/'):
                result.targets['desktop'].update(BROWSER_CASES - {'case:20_browser_workspace/01_resource_tabs'})
            elif path.startswith('talos-e2e/scenarios/20_browser_workspace/') and path.endswith('.sh'):
                target = 'case:' + path.removeprefix('talos-e2e/scenarios/').removesuffix('.sh')
                result.targets['webui' if '/01_' in path else 'desktop'].add(target)
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
            if owner == 'talos-mcp':
                result.targets['desktop'].add('case:20_browser_workspace/04_desktop_mcp')
            if owner == 'talos-browser':
                result.targets['desktop'].update(BROWSER_CASES - {'case:20_browser_workspace/01_resource_tabs'})
        elif consumers & {'talos-server', 'talos-worker-bin'}:
            result.targets['server'].update(SERVER_TARGETS.values())
            result.targets['webui'].add('case:20_browser_workspace/01_resource_tabs')
            result.targets['desktop'].update(BROWSER_CASES - {'case:20_browser_workspace/01_resource_tabs'})
        elif consumers & {'app-server', 'talos-app-server-test-client'}:
            result.targets['server'].add(SERVER_TARGETS['app-server'])
            result.targets['desktop'].update(BROWSER_CASES - {'case:20_browser_workspace/01_resource_tabs'})
    result.mcp |= 'talos-mcp' in affected
    return result
