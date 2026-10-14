import os
import subprocess
import tempfile
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
