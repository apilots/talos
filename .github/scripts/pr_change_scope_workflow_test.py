"""Execute the selected Browser job's actual shell commands with local stubs."""

import os
from pathlib import Path
import subprocess
import tempfile
import textwrap
import unittest

from pr_change_scope import ROOT, browser_e2e_plan


def workflow_script(name, following):
    workflow = (ROOT / '.github/workflows/pr-e2e.yml').read_text()
    step = workflow.split(f'- name: {name}\n', 1)[1].split(f'- name: {following}\n', 1)[0]
    return textwrap.dedent(step.split('run: |\n', 1)[1].rstrip(' \n'))


class BrowserWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'bin').mkdir()
        self.calls = self.root / 'calls'
        self.env = dict(os.environ, PATH=f'{self.root / "bin"}:{os.environ["PATH"]}', CALLS=str(self.calls))

    def executable(self, path, body):
        file = self.root / path
        file.parent.mkdir(parents=True, exist_ok=True)
        file.write_text('#!/bin/bash\nset -eu\n' + body)
        file.chmod(0o755)

    def run_script(self, script, **env):
        self.calls.unlink(missing_ok=True)
        return subprocess.run(['bash', '-c', script], cwd=self.root,
                              env={**self.env, **env}, capture_output=True, text=True)

    def test_binary_build_requires_worker_only_for_docker_targets(self):
        script = workflow_script('Build the current product binaries', 'Assemble the standard packaged Worker image')
        self.executable('bin/cargo', 'printf "%s\\n" "$@" >> "$CALLS"\n')
        for selected, worker, success in [('false', False, True), ('true', True, True), ('', False, False)]:
            with self.subTest(selected=selected):
                result = self.run_script(script, BROWSER_DOCKER=selected)
                self.assertEqual(result.returncode == 0, success, result.stderr)
                args = self.calls.read_text().splitlines() if self.calls.exists() else []
                if success:
                    self.assertEqual('talos-worker-bin' in args, worker)
                    for package in ['talos-server', 'talos-linux-sandbox', 'app-server']:
                        self.assertIn(package, args)
                else:
                    self.assertEqual(args, [])

    def test_selected_targets_run_once_and_failures_reach_the_gate(self):
        script = workflow_script('Run selected Browser workspace scenarios', 'Print browser scenario failures')
        self.executable('bin/xvfb-run', 'test "$1" = -a\nshift\nexec "$@"\n')
        self.executable('talos-e2e/run_e2e.sh', 'printf "%s %s\\n" "$1" "$2" >> "$CALLS"\n'
                        'test "$2" != "${FAIL_TARGET:-}"\n')
        for paths in [['talos-app/desktop/src/browser-host.ts'],
                      ['talos-app/desktop/src/desktop-media.ts'],
                      ['talos-app/ts-sdk/packages/core/src/client.ts']]:
            targets, _ = browser_e2e_plan(paths)
            for failing in ['', targets[0].split(':', 1)[1]]:
                with self.subTest(paths=paths, failing=failing):
                    result = self.run_script(script, BROWSER_TARGETS=' '.join(targets), FAIL_TARGET=failing)
                    self.assertEqual(result.returncode == 0, not failing, result.stderr)
                    self.assertEqual(self.calls.read_text().splitlines(),
                                     ['--' + target.replace(':', ' ', 1) for target in targets])
        for invalid in ['', 'unknown:20_browser_workspace']:
            result = self.run_script(script, BROWSER_TARGETS=invalid)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(self.calls.exists())


if __name__ == '__main__':
    unittest.main()
