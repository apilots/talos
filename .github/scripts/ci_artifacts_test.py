"""Exercise executable distribution and archive consumers without hidden compilers."""
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import ci_artifacts
import ci_run
from ci_impact import ROOT
from ci_plan import plan


class CompiledArtifactsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=ROOT / 'output')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_requested_binary_only_and_checksum_corruption(self):
        executable = self.root / 'launcher'
        executable.write_bytes(b'executable fixture')
        executable.chmod(0o755)
        ci_artifacts.pack_binary(self.root, 'sandbox', executable)
        ci_artifacts.unpack(self.root, ['sandbox'])
        self.assertEqual((self.root / 'talos-linux-sandbox-launcher').read_bytes(), executable.read_bytes())
        self.assertTrue(os.access(self.root / 'talos-linux-sandbox-launcher', os.X_OK))
        with self.assertRaisesRegex(ValueError, 'Missing compiled artifact'):
            ci_artifacts.unpack(self.root, ['server'])
        (self.root / 'binary-sandbox.tar.gz').write_bytes(b'corrupt')
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            ci_artifacts.unpack(self.root, ['sandbox'])

    def test_archive_cannot_escape_destination_or_replace_executable_with_symlink(self):
        path = self.root / 'binary-sandbox.tar.gz'
        for name, kind in [('../escape', tarfile.REGTYPE), ('talos-linux-sandbox-launcher', tarfile.SYMTYPE)]:
            with tarfile.open(path, 'w:gz') as archive:
                member = tarfile.TarInfo(name); member.type = kind
                archive.addfile(member, io.BytesIO())
            ci_artifacts.seal(path)
            with self.assertRaisesRegex(ValueError, 'Invalid binary archive'):
                ci_artifacts.unpack(self.root, ['sandbox'])

    def test_selection_preserves_each_original_test_package_set(self):
        selected = plan(['.github/release-toolchain.env'])
        groups = selected['test_build']['tests']
        for category in ('server', 'tui'):
            self.assertEqual(groups[category], selected['categories'][category]['quality'][0]['rust'])
        self.assertEqual(groups['e2e'], ['talos-tools'])
        sandbox = next(t for t in selected['categories']['server']['scenarios']['include'] if t['targets']=='group:14_sandbox')
        self.assertEqual(sandbox['binary_pattern'], 'ci-binary-sandbox')
        self.assertEqual(len(sandbox['cases']), 11)
        self.assertIn('14_sandbox/99_linux_docker.sh', sandbox['cases'])

    def test_supplied_test_archive_never_invokes_cargo_or_rebuilds_helper(self):
        tools = self.root / 'bin'; tools.mkdir()
        log = self.root / 'args.json'
        nextest = tools / 'cargo-nextest'
        nextest.write_text('#!/usr/bin/env python3\nimport json,sys\nfrom pathlib import Path\nPath('+repr(str(log))+').write_text(json.dumps(sys.argv[1:]))\n')
        nextest.chmod(0o755)
        cargo = tools / 'cargo'; cargo.write_text('#!/bin/sh\nexit 93\n');cargo.chmod(0o755)
        helper=self.root/'helper';helper.write_text('fixture');helper.chmod(0o755)
        archive=self.root/'test archive.tar.zst';archive.touch()
        env={**os.environ,'PATH':str(tools)+os.pathsep+os.environ['PATH'],
             'TALOS_TEST_ARCHIVE':str(archive),'TALOS_LINUX_SANDBOX_HELPER':str(helper)}
        subprocess.run(['just','test','sandbox_e2e_tests','--run-ignored','ignored-only'],cwd=ROOT,env=env,check=True,capture_output=True)
        args=json.loads(log.read_text())
        self.assertEqual(args[:4],['nextest','run','--no-fail-fast','--archive-file'])
        self.assertIn(str(archive),args)
        self.assertIn('sandbox_e2e_tests',args)
        self.assertIn('--workspace-remap',args)
        archive.unlink()
        result=subprocess.run(['just','test'],cwd=ROOT,env=env,capture_output=True)
        self.assertNotEqual(result.returncode,0)

    def test_corrupt_or_missing_test_archive_invalidates_build_reuse(self):
        directory = self.root / 'compiled'; directory.mkdir()
        archive = directory / 'tests-e2e.tar.zst'; archive.write_bytes(b'compiled tests')
        task = plan(['.github/release-toolchain.env'])['test_build']
        ci_run.record(directory, 'key', task)
        self.assertTrue(ci_run.cached(directory, 'key', task))
        archive.write_bytes(b'corrupt tests')
        self.assertFalse(ci_run.cached(directory, 'key', task))
        archive.unlink()
        self.assertFalse(ci_run.cached(directory, 'key', task))

    def test_missing_shared_tests_fail_before_sandbox_execution(self):
        selected=plan(['.github/release-toolchain.env'])
        sandbox=next(t for t in selected['categories']['server']['scenarios']['include'] if t['targets']=='group:14_sandbox')
        with patch.object(ci_run,'ROOT',self.root),patch.object(ci_run,'run') as execute:
            with self.assertRaisesRegex(ValueError,'Missing compiled artifact'):
                ci_run.execute(sandbox,self.root/'cache')
            execute.assert_not_called()
