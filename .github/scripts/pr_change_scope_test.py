import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest

from pr_change_scope import (
    ROOT, SCOPES, VERSION_PATTERNS, is_document,
    scope_matches, should_run, version_change_only, version_files,
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
            '.github/workflows/talos-tui-install.yml': {'tui-install'},
            '.github/release-toolchain.env': set(SCOPES),
        }
        for path, expected in cases.items():
            with self.subTest(path=path):
                self.assertEqual({s for s in SCOPES if scope_matches(s, path)}, expected)
        self.assertFalse(scope_matches('rust', '.github/workflows/talos-rust.yml', product_only=True))

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


def with_version(path, content, version='0.1.12-SNAPSHOT'):
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

    def commit(self, subject='chore(release): start v0.1.12 development'):
        self.git('add', '.')
        self.git('commit', '-qm', subject)
        return self.git('rev-parse', 'HEAD')

    def versions(self, version='0.1.12-SNAPSHOT'):
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
        self.versions('0.1.12')
        head = self.commit('chore(release): prepare v0.1.12')
        self.git('tag', '-a', 'v0.1.12', '--cleanup=verbatim', '-m', '# Talos v0.1.12\n\n## Fixes\n\n- Product fix')
        tag = self.git('rev-parse', 'v0.1.12')
        self.assertEqual(self.hook(f'refs/tags/v0.1.12 {tag} refs/tags/v0.1.12 {"0" * 40}\n'), '')
        self.assertEqual(self.hook(f'refs/heads/main {head} refs/heads/main {self.base}\n'), '')

    def test_docker_install_uses_shared_toolchain(self):
        self.write('.github/release-toolchain.env', (ROOT / '.github/release-toolchain.env').read_text())
        binary = self.write('bin/docker', '#!/bin/sh\nprintf "%s\\n" "$*" >> "$DOCKER_CALLS"\n'
                            'if [ "$1" = info ] && [ "$2" = --format ]; then echo x86_64; fi\n')
        binary.chmod(0o755)
        calls = self.root / 'docker-calls.log'
        env = dict(os.environ, PATH=f'{binary.parent}:{os.environ["PATH"]}', DOCKER_CALLS=str(calls))
        script = f'''set -euo pipefail
REPO_ROOT="{self.root}"
TALOS_E2E_DIR="{self.root}/talos-e2e"
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
        self.assertIn(f'build --build-arg RUST_VERSION={version}', calls.read_text())
        self.assertIn(f'-v {self.root}:/workspace/talos:ro', calls.read_text())
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
