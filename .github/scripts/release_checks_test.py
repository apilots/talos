import io
import os
import subprocess
import tarfile
import tempfile
import textwrap
import unittest
from pathlib import Path


class ReleaseSourceStageTests(unittest.TestCase):
    def test_source_stages_do_not_package_and_propagate_failure(self):
        script = Path("scripts/check-release.sh").read_text()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / "scripts").mkdir()
            (root / "scripts/check-release.sh").write_text(script)
            bin_dir = root / "bin"
            bin_dir.mkdir()
            for name in ["just", "pnpm", "python3"]:
                path = bin_dir / name
                path.write_text('#!/bin/sh\necho "$0 $*" >> "$CHECK_LOG"\n'
                                'if [ "${FAIL_CHECK:-0}" = 1 ] && [ "$1" = check ]; then exit 17; fi\n')
                path.chmod(0o755)
            for name in ["scripts/check-release-version-tests.sh",
                         "deploy/install/tests/install_test.sh",
                         "deploy/install/tests/release_contract_test.sh",
                         "talos-e2e/tests/tui_install_target_test.sh", "build.sh"]:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('#!/bin/sh\necho "$0" >> "$CHECK_LOG"\n')
                path.chmod(0o755)
            log = root / "checks.log"
            env = dict(os.environ, PATH=f"{bin_dir}:{os.environ['PATH']}", CHECK_LOG=str(log))
            result = subprocess.run(["bash", str(root / "scripts/check-release.sh")],
                                    env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            calls = log.read_text()
            self.assertNotIn("build.sh", calls)
            self.assertIn("just fmt-check", calls)
            self.assertIn("pnpm --dir talos-app ci:frontend-app", calls)
            log.write_text("")
            failed = subprocess.run(["bash", str(root / "scripts/check-release.sh")],
                                    env=dict(env, FAIL_CHECK="1"), capture_output=True, text=True)
            self.assertEqual(failed.returncode, 17, failed.stderr)
            self.assertNotIn("pnpm", log.read_text())


class RuntimeImageInputTests(unittest.TestCase):
    def prepare_server_tui(self, root, include_binary=True):
        source = Path('.github/workflows/release.yml').read_text()
        step = source.split('      - name: Add the existing TUI binary to the Server image input\n', 1)[1]
        step = step.split('      - name:', 1)[0]
        command = textwrap.dedent(step.split('        run: |\n', 1)[1])
        product = root / 'talos/.release-product'
        (product / 'bin').mkdir(parents=True)
        (product / 'conf').mkdir()
        (product / 'bin/talos-server').write_bytes(b'validated-server')
        (product / 'conf/talos.yaml').write_bytes(b'server-config')
        package = root / 'tui-package'
        package.mkdir()
        members = {'conf/talos.yaml': b'tui-config', 'bin/talos-server': b'unexpected-server'}
        if include_binary:
            members['bin/talos'] = b'validated-tui'
        with tarfile.open(package / 'Talos-TUI-1.2.3-linux-arm64.tar.gz', 'w:gz') as archive:
            for name, content in members.items():
                info = tarfile.TarInfo(name)
                info.size = len(content)
                info.mode = 0o755 if name.startswith('bin/') else 0o644
                archive.addfile(info, io.BytesIO(content))
        return subprocess.run(['bash', '-euo', 'pipefail', '-c', command], cwd=root,
                              env=dict(os.environ, RELEASE_VERSION='1.2.3', PACKAGE_ARCH='arm64'),
                              capture_output=True, text=True)

    def test_tui_archive_adds_only_management_binary(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.prepare_server_tui(root)
            self.assertEqual(result.returncode, 0, result.stderr)
            product = root / 'talos/.release-product'
            self.assertEqual((product / 'bin/talos').read_bytes(), b'validated-tui')
            self.assertTrue(os.access(product / 'bin/talos', os.X_OK))
            self.assertEqual((product / 'bin/talos-server').read_bytes(), b'validated-server')
            self.assertEqual((product / 'conf/talos.yaml').read_bytes(), b'server-config')

    def test_missing_tui_binary_fails_image_preparation(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            result = self.prepare_server_tui(root, include_binary=False)
            self.assertNotEqual(result.returncode, 0)
            product = root / 'talos/.release-product'
            self.assertFalse((product / 'bin/talos').exists())
            self.assertEqual((product / 'conf/talos.yaml').read_bytes(), b'server-config')
