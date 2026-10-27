import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

from pr_change_scope import ROOT, SCOPES, VERSION_PATTERNS, is_document, should_run, version_change_only, version_files
from ci_impact import impact, rust_graph, frontend_graph, SERVER_TARGETS, SERVER_BROWSER_CASE, TUI_TARGET
from ci_plan import plan, task_roots


class ChangeScopeTests(unittest.TestCase):
    def test_server_container_case_selects_packaged_products_and_web_inputs(self):
        result = plan(['talos-e2e/scenarios/20_browser_workspace/06_docker_server.sh'])
        tasks = result['categories']['server']['scenarios']['include']
        self.assertEqual(len(tasks), 1)
        task = tasks[0]
        self.assertEqual(task['cases'], ['20_browser_workspace/06_docker_server.sh'])
        self.assertEqual(task['images'], ['server'])
        self.assertEqual(set(task['binaries']), {'server', 'cli', 'worker', 'sandbox'})
        self.assertTrue(task['docker'] and task['node'])
        self.assertIn('talos-app/web', task_roots(task))
        self.assertFalse(result['categories']['desktop']['scenarios']['include'])
        helper = impact(['talos-e2e/scripts/browser-worker/server.py'])
        self.assertEqual(helper.targets['server'], {SERVER_BROWSER_CASE})
        self.assertFalse(helper.targets['desktop'] or helper.rust or helper.frontend)

    def test_shared_container_inputs_select_only_both_packaged_consumers(self):
        for path in ['build/container/install-runtime.sh', 'build/container/prepare-browser.sh']:
            selected = impact([path])
            self.assertEqual(selected.targets['server'], {SERVER_BROWSER_CASE})
            self.assertEqual(selected.targets['desktop'], {'case:20_browser_workspace/05_docker_worker'})
            self.assertFalse(selected.rust or selected.frontend or selected.installer)
        for path in ['build/server/Dockerfile', 'build/worker/browser/desktop/start.sh',
                     'talos-rs/crates/action/plugin/src/manager/mcp.rs',
                     'talos-rs/crates/app/talos-worker/src/browser_host.rs',
                     'talos-rs/apps/tui/src/input.rs']:
            self.assertIn(SERVER_BROWSER_CASE, impact([path]).targets['server'], path)

    def test_plugin_cli_selects_its_secretless_lifecycle_and_current_binaries(self):
        result = plan(['talos-rs/apps/tui/src/cli/plugin.rs'])
        tasks = result['categories']['tui']['scenarios']['include']
        task = next(task for task in tasks if '05_cli/plugin_lifecycle.sh' in task['cases'])
        self.assertTrue({'server', 'cli'} <= set(task['binaries']))

    def test_shared_plugin_cli_fixture_selects_both_worker_consumers(self):
        result = plan(['talos-e2e/scripts/plugin_cli.py'])
        for category, case in [('tui', '05_cli/plugin_lifecycle.sh'),
                               ('desktop', '20_browser_workspace/05_docker_worker.sh')]:
            tasks = result['categories'][category]['scenarios']['include']
            task = next(task for task in tasks if case in task['cases'])
            self.assertTrue({'server', 'cli'} <= set(task['binaries']))
        self.assertTrue(task['docker'])

    def test_tui_leaf_selects_only_tui(self):
        result = impact(['talos-rs/apps/tui/src/render/chat_lines.rs'])
        self.assertEqual(result.rust, {'talos-tui'})
        self.assertEqual(result.targets['tui'], {TUI_TARGET})
        self.assertFalse(result.installer or result.mcp or result.frontend)
        self.assertFalse(result.targets['desktop'] or result.targets['server'] or result.targets['webui'])

    def test_web_config_helper_and_desktop_union_stay_bounded(self):
        paths = ['talos-app/web/tailwind.config.ts', 'talos-e2e/scripts/computer_workspace/resources.py',
                 'talos-app/desktop/src/browser-host.ts']
        selected = impact(paths)
        self.assertFalse(selected.rust or selected.installer or selected.targets['server'])
        self.assertEqual(selected.frontend, {'talos-web', 'talos-desktop'})
        self.assertFalse(any('05_docker' in t for t in selected.targets['desktop']))

    def test_mixed_inputs_equal_individual_unions(self):
        paths = ['talos-rs/apps/tui/src/render/chat_lines.rs',
                 'talos-rs/crates/action/skills/src/lib.rs',
                 'talos-app/web/tailwind.config.ts', 'talos-app/desktop/src/preload.ts',
                 'talos-e2e/scripts/computer_workspace/resources.py']
        combined = impact(paths)
        separate = [impact([p]) for p in paths]
        for field in ('rust', 'frontend'):
            self.assertEqual(getattr(combined, field), set().union(*(getattr(r, field) for r in separate)))
        for category in combined.targets:
            self.assertEqual(combined.targets[category], set().union(*(r.targets[category] for r in separate)))
        self.assertEqual(combined.installer, any(r.installer for r in separate))

    def test_service_and_shared_type_changes_expand_consumers(self):
        graph = rust_graph()
        self.assertIn('talos-server', graph.closure({'talos-service'}, reverse=True))
        self.assertIn('talos-tui', graph.closure({'talos-types'}, reverse=True))
        result = impact(['talos-rs/crates/orchestration/service/src/lib.rs'])
        self.assertEqual(result.targets['server'], set(SERVER_TARGETS.values()) | {SERVER_BROWSER_CASE})
        self.assertIn(TUI_TARGET, result.targets['tui'])

    def test_server_submodules_have_focused_scenarios(self):
        for owner in ['skills', 'mcp', 'tools']:
            result = impact([f'talos-rs/crates/action/{owner}/src/lib.rs'])
            self.assertIn(f'talos-{owner}', result.rust)
            self.assertTrue(result.targets['server'])
            self.assertNotEqual(result.targets['server'], set(SERVER_TARGETS.values()))
        mcp = impact(['talos-rs/crates/action/mcp/src/lib.rs'])
        self.assertTrue(mcp.mcp)
        self.assertIn('case:20_browser_workspace/04_desktop_mcp', mcp.targets['desktop'])

    def test_platform_inputs_and_installation_are_explicit(self):
        for path in ['deploy/install/install.sh', 'talos-rs/Cargo.lock',
                     '.github/release-toolchain.env', '.github/workflows/release-install.yml']:
            self.assertTrue(impact([path]).installer, path)
        for path in ['talos-rs/apps/tui/src/render/chat_lines.rs', 'talos-rs/apps/tui/Cargo.toml',
                     'talos-rs/crates/action/mcp/src/lib.rs', 'talos-app/web/tailwind.config.ts']:
            self.assertFalse(impact([path]).installer, path)

    def test_frontend_dependencies_expand_consumers_without_rust_tests(self):
        graph = frontend_graph()
        self.assertIn('talos-web', graph.closure({'@talos/sdk-core'}, reverse=True))
        result = impact(['talos-app/ts-sdk/packages/core/src/client.ts'])
        self.assertIn('talos-desktop', result.frontend)
        self.assertFalse(result.rust or result.installer)
        self.assertIn('case:20_browser_workspace/05_docker_worker', result.targets['desktop'])

    def test_docs_templates_skip_heavy_but_unknown_product_does_not(self):
        for path in ['docs/guide.md', 'talos-rs/README.md', '.github/ISSUE_TEMPLATE/agent-task.yml']:
            self.assertFalse(should_run('ci', [path]), path)
        for path in ['talos-rs/new-build-input', 'new-runtime.py']:
            self.assertTrue(should_run('ci', [path]), path)

    def test_mixed_server_web_acceptance_executes_each_case_once(self):
        selected = plan(['talos-rs/crates/orchestration/service/src/lib.rs',
                         'talos-app/web/tailwind.config.ts'])
        cases = [case for category in selected['categories'].values()
                 for task in category['scenarios']['include'] for case in task['cases']]
        self.assertEqual(len(cases), len(set(cases)))
        self.assertIn('09_app_server/computer_deletion.sh', cases)

    def test_runtime_markdown_and_wire_contracts_are_product_inputs(self):
        result = impact(['talos-rs/crates/common/prompt/templates/system.md'])
        self.assertTrue(result.rust and result.targets['server'])
        self.assertFalse(is_document('talos-rs/crates/common/prompt/templates/system.md'))
        result = impact(['talos-rs/crates/gateway/app-protocol/src/lib.rs'])
        self.assertIn('talos-web', result.frontend)
        self.assertIn('talos-desktop', result.frontend)

    def test_full_plan_preserves_public_suites_and_declared_binaries(self):
        selected = plan(['.github/release-toolchain.env'])
        targets = {t['targets'] for c in selected['categories'].values() for t in c['scenarios']['include']}
        self.assertTrue(set(SERVER_TARGETS.values()) <= targets)
        self.assertIn(TUI_TARGET, targets)
        self.assertEqual(set(selected['build']['binaries']), {'server','cli','app_server','test_client','worker','sandbox'})
        self.assertTrue(selected['installer'] and selected['mcp'])

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
            self.assertEqual(self.classify(event, head), 'run_workflow=false')
        self.assertEqual(self.classify('push', head, 'frontend'), 'run_workflow=false')

    def test_first_push_uses_empty_tree_and_checks_product(self):
        self.base = '0' * 40
        head = self.git('rev-parse', 'HEAD')
        self.assertEqual(self.classify('push', head, 'rust'), 'run_workflow=true')

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

    def test_ci_diff_exports_bounded_union(self):
        self.write('talos-app/web/tailwind.config.ts', 'export default {};\n')
        self.write('talos-e2e/scripts/computer_workspace/resources.py', '# changed\n')
        head = self.commit('fix(web): adjust build and acceptance')
        self.classify('pull_request', head, 'ci')
        values = dict(line.split('=', 1) for line in (self.root / 'scope.outputs').read_text().splitlines())
        selected = json.loads(values['plan'])
        self.assertFalse(selected['installer'])
        self.assertFalse(selected['categories']['server']['quality'])
        self.assertFalse(selected['categories']['server']['scenarios']['include'])
        self.assertTrue(selected['categories']['webui']['quality'])

    def test_rename_and_delete_keep_old_and_new_owners(self):
        self.write('talos-rs/apps/tui/src/fixture.rs', 'pub fn fixture() {}\n')
        self.base = self.commit('test: initialize path')
        self.git('mv', 'talos-rs/apps/tui/src/fixture.rs', 'talos-app/web/fixture.ts')
        head = self.commit('refactor: move owned file')
        self.assertEqual(self.classify('pull_request', head, 'rust'), 'run_workflow=true')
        self.assertEqual(self.classify('pull_request', head, 'frontend'), 'run_workflow=true')
        self.git('rm', 'talos-app/web/fixture.ts')
        deleted = self.commit('refactor: remove owned file')
        self.assertEqual(self.classify('push', deleted, 'rust'), 'run_workflow=true')

    def test_removed_version_file_is_not_version_only(self):
        (self.root / 'talos-app/web/package.json').unlink()
        head = self.commit('fix(web): update packaging')
        self.assertEqual(self.classify('push', head, 'frontend'), 'run_workflow=true')

    def prepare_hook(self):
        for path in ['scripts/run-pre-push-checks.sh', 'scripts/check-release-version.sh',
                     '.github/scripts/pr_change_scope.py', '.github/scripts/ci_impact.py']:
            self.write(path, (ROOT / path).read_text())
        for name in ['check_changed_files', 'check_error_contract', 'check_error_contract_tests']:
            self.write(f'scripts/{name}.py', 'print("File/error policy checked")\n')
        self.write('scripts/check-commit-message.sh', '#!/bin/sh\nexit 0\n').chmod(0o755)
        manifests = ['talos-rs/Cargo.toml', 'talos-app/package.json']
        manifests += [path + '/Cargo.toml' for path in rust_graph().roots.values()]
        manifests += [path + '/package.json' for path in frontend_graph().roots.values()]
        for path in manifests:
            self.write(path, (ROOT / path).read_text())
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

    def test_docker_install_consumes_current_release_product_and_propagates_failure(self):
        binary = self.write('bin/docker', '#!/bin/sh\nprintf "%s\\n" "$*" >> "$DOCKER_CALLS"\n'
                            'if [ "$1" = image ]; then exit 1; fi\n'
                            'if [ "$1" = info ] && [ "$2" = --format ]; then echo x86_64; fi\n'
                            'if [ "$1" = run ]; then exit "${DOCKER_RUN_STATUS:-0}"; fi\n')
        binary.chmod(0o755)
        self.write('products/tui/SHA256SUMS', 'current-product-checksum')
        calls = self.root / 'docker-calls.log'
        env = dict(os.environ, PATH=f'{binary.parent}:{os.environ["PATH"]}', DOCKER_CALLS=str(calls),
                   TUI_INSTALL_PRODUCT_DIR=str(self.root/'products'))
        script = f'''set -euo pipefail
source "{ROOT}/talos-e2e/scripts/common.sh"
export TALOS_E2E_RESOURCE_ROOT="{self.root}/resources"
export TALOS_E2E_RUN_ID=product-test TALOS_E2E_CASE_ID=15_tui_install/02_linux_docker
REPO_ROOT="{self.root}"
TALOS_E2E_DIR="{ROOT}/talos-e2e"
TALOS_E2E_TMP_ROOT="{self.root}/tmp"
SCENARIO_REPORT_DIR="{self.root}/reports"
require_command() {{ command -v "$1" >/dev/null; }}
source "{ROOT}/talos-e2e/scripts/tui_install.sh"
run_linux_docker_tui_install
'''
        def invoke(**overrides):
            return subprocess.run(['bash', '-c', script], env=dict(env, **overrides), capture_output=True, text=True)
        result = invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        recorded = calls.read_text()
        self.assertIn(f'-v {self.root}/products:/opt/talos-product:ro', recorded)
        self.assertIn('-e TUI_INSTALL_PRODUCT_DIR=/opt/talos-product', recorded)
        self.assertIn(f'-v {self.root}:/workspace/talos:ro', recorded)
        self.assertNotIn('volume create', recorded)
        self.assertNotIn('RUST_VERSION', recorded)
        self.assertNotIn('/talos-rs/target', recorded)
        self.assertNotEqual(invoke(DOCKER_RUN_STATUS='42').returncode, 0)
        calls.unlink()
        result = invoke(TUI_INSTALL_PRODUCT_DIR='')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('requires TUI_INSTALL_PRODUCT_DIR', result.stderr)
        self.assertNotIn('build ', calls.read_text())
        (self.root/'products/tui/SHA256SUMS').unlink()
        self.assertNotEqual(invoke().returncode, 0)

    def test_product_change_in_mixed_ref_push_keeps_source_checks(self):
        self.prepare_hook()
        self.git('tag', 'fixture-base')
        self.write('talos-rs/bin/talos-server/src/fixture.rs', 'pub fn changed() {}\n')
        head = self.commit('fix: preserve changed product checks')
        refs = f'refs/tags/fixture-base {self.base} refs/tags/fixture-base {"0" * 40}\n'
        refs += f'refs/heads/main {head} refs/heads/main {self.base}\n'
        calls = self.hook(refs)
        self.assertIn('just check', calls)
        self.assertNotIn('pnpm', calls)


if __name__ == '__main__':
    unittest.main()
