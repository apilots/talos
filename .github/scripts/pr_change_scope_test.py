import json
import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import tomllib
import unittest

from pr_change_scope import (
    ROOT, SCOPES, VERSION_PATTERNS, is_document,
    browser_e2e_applies, browser_e2e_plan, e2e_plan, scope_matches, should_run, version_change_only, version_files,
)


class ChangeScopeTests(unittest.TestCase):
    def test_ci_fetches_committed_graph_without_regeneration(self):
        for workflow in ['talos-rust', 'pr-e2e', 'talos-tui-install', 'trusted-e2e', 'release-source']:
            content = (ROOT / f'.github/workflows/{workflow}.yml').read_text()
            self.assertNotIn('cargo generate-lockfile', content)
            self.assertIn('cargo fetch --locked --manifest-path talos-rs/Cargo.toml', content)
        installer = (ROOT / '.github/workflows/talos-tui-install.yml').read_text()
        self.assertEqual(installer.count('cargo fetch --locked'), 1)

    def test_documents_and_unrelated_files(self):
        for scope in SCOPES:
            for path in ['docs/guide.md', 'talos-rs/README.md', 'spec/design.rst']:
                self.assertTrue(is_document(path))
                self.assertFalse(should_run(scope, [path]))
            self.assertFalse(should_run(scope, ['.github/ISSUE_TEMPLATE/agent-task.yml']))

    def test_product_and_ci_owners(self):
        cases = {
            'talos-rs/crates/base/src/lib.rs': {'rust', 'pr-e2e', 'tui-install'},
            'talos-app/web/src/app.tsx': {'frontend', 'pr-e2e'},
            'conf/talos-prod.yaml': {'pr-e2e', 'tui-install'},
            'deploy/install/install.sh': {'tui-install'},
            'talos-e2e/docker/tui-install-ubuntu-24.04.Dockerfile': {'pr-e2e', 'tui-install'},
            'build.sh': {'pr-e2e', 'tui-install'},
            'justfile': {'rust', 'pr-e2e'},
            'scripts/check_error_contract.py': {'rust', 'frontend'},
            '.github/workflows/talos-rust.yml': {'rust'},
            '.github/workflows/talos-frontend-app.yml': {'frontend'},
            '.github/workflows/pr-e2e.yml': {'pr-e2e'},
            '.github/scripts/pr_change_scope.py': {'pr-e2e'},
            '.github/scripts/pr_change_scope_test.py': {'pr-e2e'},
            '.github/workflows/talos-tui-install.yml': {'tui-install'},
            '.github/release-toolchain.env': set(SCOPES),
        }
        for path, expected in cases.items():
            with self.subTest(path=path):
                self.assertEqual({s for s in SCOPES if scope_matches(s, path)}, expected)
        self.assertFalse(scope_matches('rust', '.github/workflows/talos-rust.yml', product_only=True))

    def test_web_source_changes_keep_real_browser_coverage(self):
        paths = ['talos-app/web/src/pages/computers/ComputersPage.tsx',
                 'talos-app/web/src/pages/computers/computers.css',
                 'talos-app/web/src/pages/computers/ComputersPage.test.tsx',
                 'talos-app/web/public/app-icon.svg']
        self.assertTrue(should_run('frontend', paths))
        self.assertTrue(should_run('pr-e2e', paths))
        contracts, matrix = e2e_plan(paths)
        self.assertFalse(contracts)
        self.assertEqual([scenario['targets'] for scenario in matrix['include']],
                         ['case:09_app_server/computer_deletion',
                          'case:20_browser_workspace/01_resource_tabs'])

    def test_resource_workspaces_keep_hosted_product_e2e_without_docker(self):
        for path in ["talos-app/web/src/features/web-preview/WebPagePreview.tsx",
                     "talos-app/web/src/features/files/components/MarkdownFilePreview.tsx"]:
            contracts, matrix = e2e_plan([path])
            self.assertFalse(contracts)
            self.assertEqual(matrix["include"][1]["targets"], "case:20_browser_workspace/01_resource_tabs")
            self.assertFalse(browser_e2e_applies([path]))

    def test_heavy_browser_owners_do_not_repeat_group20_in_generic_matrix(self):
        for path in ['talos-app/desktop/src/browser-host.ts',
                     'talos-app/web/src/app/layouts/AppLayout.tsx',
                     'talos-e2e/scenarios/20_browser_workspace/05_docker_worker.sh']:
            with self.subTest(path=path):
                self.assertTrue(browser_e2e_applies([path]))
                _, matrix = e2e_plan([path])
                self.assertFalse(any('20_browser_workspace' in item['targets']
                                     for item in matrix['include']))

    def test_non_web_or_mixed_inputs_keep_native_contracts_and_full_scenarios(self):
        inputs = ['talos-app/desktop/package.json', 'talos-app/desktop/vite.config.ts',
                  'talos-app/ts-sdk/packages/core/src/client.ts',
                  'talos-app/pnpm-lock.yaml', 'talos-app/package.json',
                  'talos-app/web/package.json', 'talos-app/web/vite.config.ts',
                  'talos-app/web/index.html', 'talos-app/web/new-build-input',
                  'talos-rs/crates/service/src/lib.rs', 'conf/talos.yaml',
                  'talos-e2e/scenarios/09_app_server/computer_deletion.sh',
                  '.github/release-toolchain.env']
        for path in inputs:
            for paths in ([path], ['talos-app/web/src/app.tsx', path]):
                with self.subTest(paths=paths):
                    self.assertTrue(should_run('pr-e2e', paths))
                    contracts, matrix = e2e_plan(paths)
                    self.assertTrue(contracts)
                    self.assertEqual({s['name'] for s in matrix['include']},
                                     {'health-and-model-catalog', 'app-server',
                                      'computer-registration', 'files', 'sandbox'})
        self.assertTrue(e2e_plan([])[0])

    def test_desktop_sources_select_native_acceptance_without_generic_or_docker_work(self):
        sources = ['browser-host.ts', 'browser-host.cancellation.test.ts', 'browser-input.ts',
                   'browser-website.ts', 'main.ts']
        paths = [f'talos-app/desktop/src/{name}' for name in sources]
        paths += ['talos-app/desktop/resources/browser-window/chrome.js', 'docs/browser.md']
        expected = [f'case:20_browser_workspace/{name}' for name in
                    ['02_desktop_resources', '03_desktop_browser', '04_desktop_mcp']]
        for routing in [[], ['.github/workflows/pr-e2e.yml', '.github/scripts/pr_change_scope.py',
                             '.github/scripts/pr_change_scope_test.py',
                             '.github/scripts/pr_change_scope_workflow_test.py']]:
            with self.subTest(routing=routing):
                self.assertEqual(e2e_plan(paths + routing), (False, {'include': []}))
                self.assertEqual(browser_e2e_plan(paths + routing), (expected, False))

    def test_desktop_media_owners_retain_docker_acceptance(self):
        for owner in ['desktop-media.ts', 'desktop-ipc.ts', 'desktop-server-requests.ts',
                      'ipc-guard.ts', 'ipc.ts', 'preload.ts', 'app-server-controller.ts']:
            with self.subTest(owner=owner):
                paths = [f'talos-app/desktop/src/{owner}']
                self.assertEqual(e2e_plan(paths), (False, {'include': []}))
                targets, docker = browser_e2e_plan(paths)
                self.assertTrue(docker)
                self.assertEqual(len(targets), 4)
                self.assertEqual(targets[-1], 'case:20_browser_workspace/05_docker_worker')

    def test_routing_only_and_mixed_desktop_inputs_remain_conservative(self):
        for path in ['.github/workflows/pr-e2e.yml', '.github/scripts/pr_change_scope.py',
                     '.github/scripts/pr_change_scope_test.py']:
            self.assertTrue(e2e_plan([path])[0])
            self.assertEqual(browser_e2e_plan([path]), (['group:20_browser_workspace'], True))
        for path in ['talos-app/web/src/app.tsx', 'talos-app/ts-sdk/packages/core/src/client.ts',
                     'talos-app/pnpm-lock.yaml', 'talos-app/desktop/vite.config.ts',
                     'talos-app/shared/browser-home/home.html', 'build/worker/Dockerfile',
                     'talos-rs/crates/service/src/lib.rs', 'conf/talos.yaml',
                     '.github/release-toolchain.env']:
            paths = ['talos-app/desktop/src/browser-host.ts', path]
            self.assertTrue(e2e_plan(paths)[0], path)
            self.assertEqual(len(e2e_plan(paths)[1]['include']), 5)
            self.assertEqual(browser_e2e_plan(paths), (['group:20_browser_workspace'], True))

    def test_shared_browser_assets_are_build_inputs(self):
        for path in ["talos-app/web/public/talos-logo.svg", "talos-app/shared/browser-home/home.html", "talos-app/desktop/resources/browser-window/chrome.js"]:
            self.assertTrue(browser_e2e_applies([path]), path)

    def test_browser_scope_retains_owners_and_skips_unrelated_web_edits(self):
        for path in ["talos-app/web/src/pages/projects/ProjectsPage.tsx", "talos-app/web/public/icon.svg", "docs/browser.md"]:
            self.assertFalse(browser_e2e_applies([path]))
        for path in ["talos-app/web/src/features/computers/ComputerDesktop.tsx",
                     "talos-app/web/src/features/settings/components/mcp/BrowserSettings.tsx",
                     "talos-app/web/src/shared/api/client.ts", "talos-app/web/src/shared/ui/Button.tsx",
                     "talos-app/ts-sdk/packages/core/src/client.ts", "talos-app/desktop/src/main.ts",
                     "talos-rs/crates/action/browser/src/runtime.rs", "build/worker/Dockerfile",
                     "conf/talos-dev.yaml"]:
            self.assertTrue(browser_e2e_applies(["talos-app/web/public/icon.svg", path]), path)
        self.assertTrue(browser_e2e_applies([]))

    def test_only_owned_version_values_are_normalized(self):
        for path in version_files():
            before = (ROOT / path).read_text()
            after = with_version(path, before)
            with self.subTest(path=path):
                self.assertNotEqual(before, after)
                self.assertTrue(version_change_only(path, before, after))
                if path == 'talos-rs/Cargo.lock':
                    registry = next(p for p in tomllib.loads(after)['package'] if 'source' in p)
                    mixed = after.replace(f'checksum = "{registry["checksum"]}"', 'checksum = "changed"', 1)
                else:
                    mixed = json.dumps({**json.loads(after), 'private': not bool(json.loads(after).get('private'))}) if path.endswith('/package.json') else after + '\n# product change\n'
                self.assertFalse(version_change_only(path, before, mixed))
        cargo = '[dependencies]\nexample = "0.1.11"\n[workspace.package]\nversion = "0.1.11"\n'
        self.assertFalse(version_change_only('talos-rs/Cargo.toml', cargo, cargo.replace('example = "0.1.11"', 'example = "0.1.12"')))
        package = '{"version":"0.1.11","dependencies":{"x":"1"}}'
        self.assertFalse(version_change_only('talos-app/package.json', package, package.replace('"x":"1"', '"x":"2"')))

    def test_lock_keeps_registry_identity_checksums_and_edges(self):
        before = '''version = 4
[[package]]
name = "app"
version = "0.1.11"
dependencies = ["helper 0.1.11", "external 0.1.11"]
[[package]]
name = "helper"
version = "0.1.11"
[[package]]
name = "external"
version = "0.1.11"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "original"
'''
        path = 'talos-rs/Cargo.lock'
        after = with_version(path, before)
        self.assertTrue(version_change_only(path, before, after))
        for changed in [after.replace('external 0.1.11', 'external 0.1.12'),
                        after.replace('checksum = "original"', 'checksum = "changed"'),
                        after.replace('name = "helper"', 'name = "replacement"'),
                        after.replace('version = "0.1.11"', 'version = "0.1.12"'),
                        after.replace('registry+', 'git+')]:
            self.assertFalse(version_change_only(path, before, changed))


class E2EGateTests(unittest.TestCase):
    def test_gate_requires_selected_checks_and_rejects_invalid_outputs(self):
        workflow = (ROOT / '.github/workflows/pr-e2e.yml').read_text()
        gate = workflow.split('name: Require public PR contract tests and scenarios', 1)[1]
        script = textwrap.dedent(gate.split('run: |\n', 1)[1].split('\n  browser-worker:', 1)[0])
        defaults = dict(SCOPE_RESULT='success', RUN_WORKFLOW='true', RUN_CONTRACTS='true',
                        RUN_SCENARIOS='true', CONTRACT_RESULT='success', SCENARIO_RESULT='success', RUN_BROWSER='true', BROWSER_RESULT='success')
        cases = [({}, True),
                 ({'RUN_CONTRACTS':'false','CONTRACT_RESULT':'skipped',
                   'RUN_SCENARIOS':'false','SCENARIO_RESULT':'skipped'}, True),
                 ({'RUN_SCENARIOS':'false','SCENARIO_RESULT':'failure'}, False),
                 ({'RUN_SCENARIOS':''}, False),
                 ({'RUN_CONTRACTS':'false','CONTRACT_RESULT':'skipped',
                   'RUN_SCENARIOS':'false','SCENARIO_RESULT':'skipped',
                   'RUN_BROWSER':'false','BROWSER_RESULT':'skipped'}, False), ({'RUN_BROWSER':'false','BROWSER_RESULT':'skipped'}, True),
                 ({'RUN_BROWSER':'true','BROWSER_RESULT':'skipped'}, False),
                 ({'RUN_BROWSER':'false','BROWSER_RESULT':'failure'}, False),
                 ({'RUN_BROWSER':''}, False), ({'RUN_CONTRACTS': 'false', 'CONTRACT_RESULT': 'skipped'}, True),
                 ({'CONTRACT_RESULT': 'skipped'}, False), ({'CONTRACT_RESULT': 'failure'}, False),
                 ({'SCENARIO_RESULT': 'skipped'}, False), ({'SCENARIO_RESULT': 'failure'}, False),
                 ({'RUN_CONTRACTS': 'false', 'CONTRACT_RESULT': 'failure'}, False),
                 ({'RUN_CONTRACTS': 'false', 'SCENARIO_RESULT': 'skipped'}, False),
                 ({'RUN_CONTRACTS': ''}, False), ({'RUN_WORKFLOW': ''}, False),
                 ({'SCOPE_RESULT': 'failure'}, False),
                 ({'RUN_WORKFLOW': 'false', 'CONTRACT_RESULT': 'skipped', 'SCENARIO_RESULT': 'skipped'}, True)]
        for changes, success in cases:
            with self.subTest(changes=changes):
                result = subprocess.run(['bash', '-c', script], capture_output=True, text=True,
                                        env={**os.environ, **defaults, **changes})
                self.assertEqual(result.returncode == 0, success, result.stdout + result.stderr)


def next_fixture_version():
    current = tomllib.loads((ROOT / 'talos-rs/Cargo.toml').read_text())['workspace']['package']['version']
    major, minor, patch = map(int, current.split('-', 1)[0].split('.'))
    return f'{major}.{minor}.{patch + 1}-SNAPSHOT'


def with_version(path, content, version=None):
    if version is None:
        version = next_fixture_version()
    if path == 'talos-rs/Cargo.lock':
        import re
        local = [p for p in tomllib.loads(content)['package'] if 'source' not in p]
        blocks = content.split('[[package]]')
        for i in range(1, len(blocks)):
            if 'source' not in tomllib.loads(blocks[i]):
                blocks[i] = re.sub(r'(?m)^version = "[^"]+"', f'version = "{version}"', blocks[i])
        result = '[[package]]'.join(blocks)
        for package in local:
            result = result.replace(f'"{package["name"]} {package["version"]}"',
                                    f'"{package["name"]} {version}"')
        return result
    if path.endswith('/package.json'):
        data = json.loads(content)
        data['version'] = version
        return json.dumps(data) + '\n'
    if path.startswith('deploy/'):
        version = version.removesuffix('-SNAPSHOT')
    import re
    return re.sub(VERSION_PATTERNS[path], lambda m: m[1] + version + m[3], content)


class GitScopeTests(unittest.TestCase):
    def setUp(self):
        (ROOT / 'output').mkdir(exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(dir=ROOT / 'output')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.git('init', '-q', '-b', 'main')
        self.git('config', 'user.name', 'CI Contract Test')
        self.git('config', 'user.email', 'ci-contract@example.com')
        self.git('config', 'core.hooksPath', '/dev/null')
        for path in version_files():
            self.write(path, (ROOT / path).read_text())
        self.base = self.commit('chore: initialize fixture')
        self.git('update-ref', 'refs/remotes/origin/main', self.base)

    def git(self, *args):
        return subprocess.check_output(['git', *args], cwd=self.root, text=True).strip()

    def write(self, path, text):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text(text)
        return file

    def commit(self, subject='chore(release): update version fixture'):
        self.git('add', '.')
        self.git('commit', '-qm', subject)
        return self.git('rev-parse', 'HEAD')

    def versions(self, version=None):
        for path in version_files():
            file = self.root / path
            file.write_text(with_version(path, file.read_text(), version))

    def classify(self, event, head, scope='tui-install'):
        output = self.root / 'scope.outputs'
        env = dict(os.environ, GITHUB_EVENT_NAME=event, PR_BASE_SHA=self.base,
                   PR_HEAD_SHA=head, PUSH_BASE_SHA=self.base, PUSH_HEAD_SHA=head,
                   GITHUB_OUTPUT=str(output))
        result = subprocess.run(['python3', str(ROOT / '.github/scripts/pr_change_scope.py'), scope],
                                cwd=self.root, env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        return output.read_text().splitlines()[-1]

    def test_pr_and_main_skip_actual_version_only_diff(self):
        self.versions()
        self.write('docs/releases/v0.1.11.md', '# Notes\n')
        head = self.commit()
        for scope in SCOPES:
            for event in ['pull_request', 'push']:
                self.assertEqual(self.classify(event, head, scope), 'run_workflow=false')
        self.assertEqual(self.classify('workflow_dispatch', head), 'run_workflow=true')

    def test_mixed_changes_still_run_with_release_subject(self):
        self.versions()
        file = self.root / 'talos-rs/apps/tui/src/utils.rs'
        file.write_text(file.read_text() + '\npub fn product_change() {}\n')
        head = self.commit()
        for event in ['pull_request', 'push']:
            self.assertEqual(self.classify(event, head, 'rust'), 'run_workflow=true')
            self.assertEqual(self.classify(event, head), 'run_workflow=true')
        self.assertEqual(self.classify('push', head, 'frontend'), 'run_workflow=false')

    def test_dependency_lock_changes_run_affected_jobs(self):
        self.versions()
        path = self.root / 'talos-rs/Cargo.lock'
        lock = path.read_text()
        registry = next(p for p in tomllib.loads(lock)['package'] if 'source' in p)
        path.write_text(lock.replace(f'checksum = "{registry["checksum"]}"', 'checksum = "changed"', 1))
        head = self.commit()
        for event in ['pull_request', 'push']:
            for scope in ['rust', 'pr-e2e', 'tui-install']:
                self.assertEqual(self.classify(event, head, scope), 'run_workflow=true')
            self.assertEqual(self.classify(event, head, 'frontend'), 'run_workflow=false')

    def test_installer_changes_run_in_pr_and_main(self):
        self.write('deploy/install/install.sh', '#!/bin/sh\nexit 0\n')
        head = self.commit('fix(install): validate installer input')
        for event in ['pull_request', 'push']:
            self.assertEqual(self.classify(event, head), 'run_workflow=true')

    def test_web_and_mixed_git_diffs_export_the_selected_matrix(self):
        self.write('talos-app/web/src/pages/computers.css', 'body {}\n')
        self.write('docs/ui.md', '# UI\n')
        head = self.commit('fix(web): refine layout')
        for event in ['pull_request', 'push']:
            self.classify(event, head, 'pr-e2e')
            values = dict(line.split('=', 1) for line in (self.root / 'scope.outputs').read_text().splitlines())
            self.assertEqual(values['run_workflow'], 'true')
            self.assertEqual(values['run_contracts'], 'false')
            self.assertEqual(len(json.loads(values['scenario_matrix'])['include']), 2)
        self.write('talos-app/web/vite.config.ts', 'export default {}\n')
        head = self.commit('build(web): update build inputs')
        self.classify('pull_request', head, 'pr-e2e')
        values = dict(line.split('=', 1) for line in (self.root / 'scope.outputs').read_text().splitlines())
        self.assertEqual(values['run_contracts'], 'true')
        self.assertEqual(len(json.loads(values['scenario_matrix'])['include']), 5)

    def test_desktop_git_diffs_export_native_plan_and_mixed_sdk_restores_full_route(self):
        for path in ['talos-app/desktop/src/browser-host.ts', '.github/scripts/pr_change_scope.py',
                     '.github/workflows/pr-e2e.yml', 'spec/browser/design.md']:
            self.write(path, '# fixture change\n')
        head = self.commit('ci: scope Desktop acceptance')
        for event in ['pull_request', 'push']:
            self.classify(event, head, 'pr-e2e')
            values = dict(line.split('=', 1) for line in (self.root / 'scope.outputs').read_text().splitlines())
            self.assertEqual({key: values[key] for key in
                              ['run_workflow', 'run_contracts', 'run_scenarios', 'run_browser', 'browser_docker']},
                             dict(run_workflow='true', run_contracts='false', run_scenarios='false',
                                  run_browser='true', browser_docker='false'))
            self.assertEqual(json.loads(values['scenario_matrix']), {'include': []})
            self.assertEqual(values['browser_targets'].split(), browser_e2e_plan(['talos-app/desktop/src/browser-host.ts'])[0])
        self.write('talos-app/ts-sdk/packages/core/src/client.ts', 'export {};\n')
        head = self.commit('fix(sdk): update client')
        self.classify('pull_request', head, 'pr-e2e')
        values = dict(line.split('=', 1) for line in (self.root / 'scope.outputs').read_text().splitlines())
        for key in ['run_contracts', 'run_scenarios', 'run_browser', 'browser_docker']:
            self.assertEqual(values[key], 'true', key)
        self.assertEqual(values['browser_targets'], 'group:20_browser_workspace')
        self.assertEqual(len(json.loads(values['scenario_matrix'])['include']), 5)

    def test_removed_version_file_is_not_version_only(self):
        (self.root / 'talos-app/web/package.json').unlink()
        head = self.commit('fix(web): update packaging')
        self.assertEqual(self.classify('push', head, 'frontend'), 'run_workflow=true')

    def prepare_hook(self):
        for path in ['scripts/run-pre-push-checks.sh', 'scripts/check-release-version.sh',
                     '.github/scripts/pr_change_scope.py']:
            self.write(path, (ROOT / path).read_text())
        for name in ['check_changed_files', 'check_error_contract', 'check_error_contract_tests']:
            self.write(f'scripts/{name}.py', 'print("File/error policy checked")\n')
        self.write('scripts/check-commit-message.sh', '#!/bin/sh\nexit 0\n').chmod(0o755)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        for name in ['pnpm', 'just']:
            self.write(f'bin/{name}', f'#!/bin/sh\nprintf "%s\\n" "{name} $*" >> "$HOOK_CALLS"\n').chmod(0o755)
        self.base = self.commit('ci: prepare hook fixture')
        self.git('update-ref', 'refs/remotes/origin/main', self.base)

    def hook(self, refs):
        calls = self.root / 'calls.log'
        result = subprocess.run(['bash', 'scripts/run-pre-push-checks.sh'], cwd=self.root,
                                input=refs, capture_output=True, text=True,
                                env=dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}',
                                         HOOK_CALLS=str(calls)))
        self.assertEqual(result.returncode, 0, result.stderr)
        return calls.read_text() if calls.exists() else ''

    def test_tag_and_metadata_pushes_do_not_repeat_source_checks(self):
        self.prepare_hook()
        version = next_fixture_version().removesuffix('-SNAPSHOT')
        self.versions(version)
        head = self.commit(f'chore(release): prepare v{version}')
        self.git('tag', '-a', f'v{version}', '--cleanup=verbatim', '-m', f'# Talos v{version}\n\n## Fixes\n\n- Product fix')
        tag = self.git('rev-parse', f'v{version}')
        self.assertEqual(self.hook(f'refs/tags/v{version} {tag} refs/tags/v{version} {"0" * 40}\n'), '')
        self.assertEqual(self.hook(f'refs/heads/main {head} refs/heads/main {self.base}\n'), '')

    def test_docker_install_uses_shared_toolchain(self):
        self.write('.github/release-toolchain.env', (ROOT / '.github/release-toolchain.env').read_text())
        binary = self.write('bin/docker', '#!/bin/sh\nprintf "%s\\n" "$*" >> "$DOCKER_CALLS"\n'
                            'if [ "$1" = image ]; then exit 1; fi\n'
                            'if [ "$1" = info ] && [ "$2" = --format ]; then echo x86_64; fi\n')
        binary.chmod(0o755)
        calls = self.root / 'docker-calls.log'
        env = dict(os.environ, PATH=f'{binary.parent}:{os.environ["PATH"]}', DOCKER_CALLS=str(calls))
        script = f'''set -euo pipefail
source "{ROOT}/talos-e2e/scripts/common.sh"
export TALOS_E2E_RESOURCE_ROOT="{self.root}/resources"
export TALOS_E2E_RUN_ID=toolchain-test TALOS_E2E_CASE_ID=15_tui_install/02_linux_docker
REPO_ROOT="{self.root}"
TALOS_E2E_DIR="{ROOT}/talos-e2e"
TALOS_E2E_TMP_ROOT="{self.root}/tmp"
SCENARIO_REPORT_DIR="{self.root}/reports"
require_command() {{ command -v "$1" >/dev/null; }}
source "{ROOT}/talos-e2e/scripts/tui_install.sh"
run_linux_docker_tui_install
'''
        result = subprocess.run(['bash', '-c', script], env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        version = next(line.split('=', 1)[1] for line in (ROOT / '.github/release-toolchain.env').read_text().splitlines()
                       if line.startswith('RELEASE_RUST_VERSION='))
        self.assertIn(f'--build-arg RUST_VERSION={version}', calls.read_text())
        self.assertIn('build --label love.talos.e2e.run=toolchain-test', calls.read_text())
        self.assertIn(f'-v {self.root}:/workspace/talos:ro', calls.read_text())
        self.assertIn('volume create --label love.talos.e2e.run=toolchain-test tui-install-target-', calls.read_text())
        self.assertRegex(calls.read_text(), r'-v tui-install-target-toolchain-test-\d+:/workspace/talos/talos-rs/target')
        self.assertNotIn(f'-v {self.root}/tmp', calls.read_text())
        self.assertNotIn('/workspace/talos/talos-rs/Cargo.lock', calls.read_text())
        self.assertEqual((self.root / 'talos-rs/Cargo.lock').read_bytes(),
                         (ROOT / 'talos-rs/Cargo.lock').read_bytes())
        self.assertFalse((self.root / 'tmp/tui-install-linux-target-x86_64/Cargo.lock').exists())
        (self.root / 'talos-rs/Cargo.lock').unlink()
        calls.unlink()
        missing = subprocess.run(['bash', '-c', script], env=env, capture_output=True, text=True)
        self.assertNotEqual(missing.returncode, 0)
        self.assertIn('committed talos-rs/Cargo.lock is required', missing.stderr)
        self.assertNotIn('build', calls.read_text())
        self.assertFalse((self.root / 'talos-rs/Cargo.lock').exists())

    def test_product_change_in_mixed_ref_push_keeps_source_checks(self):
        self.prepare_hook()
        self.git('tag', 'fixture-base')
        self.write('talos-rs/crates/example/src/lib.rs', 'pub fn changed() {}\n')
        head = self.commit('fix: preserve changed product checks')
        refs = f'refs/tags/fixture-base {self.base} refs/tags/fixture-base {"0" * 40}\n'
        refs += f'refs/heads/main {head} refs/heads/main {self.base}\n'
        calls = self.hook(refs)
        self.assertIn('just check', calls)
        self.assertNotIn('pnpm', calls)


if __name__ == '__main__':
    unittest.main()
