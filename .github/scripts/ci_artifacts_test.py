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
from ci_plan import plan, scenario_task


class CompiledArtifactsTests(unittest.TestCase):
    def setUp(self):
        (ROOT / 'output').mkdir(exist_ok=True)
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

    def test_server_image_consumes_current_products_without_a_rust_rebuild(self):
        task = scenario_task('server', 'case:20_browser_workspace/06_docker_server')
        products = self.root / 'output/ci-binaries'
        products.mkdir(parents=True)
        for name in ('talos', 'talos-server', 'talos-worker', 'talos-linux-sandbox-launcher'):
            binary = products / name
            binary.write_text('current run ' + name)
            binary.chmod(0o755)
        web = self.root / 'talos-app/web/dist'
        web.mkdir(parents=True)
        (web / 'index.html').write_text('current production WebUI')
        with patch.object(ci_run, 'ROOT', self.root), patch.object(ci_run, 'run') as execute:
            ci_run.execute(task, self.root / 'cache')
        commands = [call.args for call in execute.call_args_list]
        self.assertEqual(commands[:2], [
            ('pnpm', '--dir', 'talos-app', '--filter', 'talos-web', 'build'),
            ('docker', 'build', '--build-arg', 'PRODUCT_STAGE=packaged', '-f',
             'build/server/Dockerfile', '-t', 'talos-server:browser-e2e', '..'),
        ])
        self.assertFalse(any(command[0] == 'cargo' or command[:2] == ('just', 'build') for command in commands))
        for binary in products.iterdir():
            self.assertEqual((self.root / '.release-product/bin' / binary.name).read_bytes(), binary.read_bytes())
        self.assertEqual((self.root / '.release-product/webui/index.html').read_text(), 'current production WebUI')
        self.assertEqual(execute.call_args.kwargs['env']['TALOS_E2E_SERVER_IMAGE'], 'talos-server:browser-e2e')

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

    def test_sandbox_requires_versioned_digest_before_execution(self):
        selected = plan(['.github/release-toolchain.env'])
        task = next(t for t in selected['categories']['server']['scenarios']['include'] if t['targets']=='group:14_sandbox')
        archive = self.root / 'output/ci-tests/tests-e2e.tar.zst'
        archive.parent.mkdir(parents=True)
        archive.write_bytes(b'compiled tests'); ci_artifacts.seal(archive)
        helper = self.root / 'output/ci-binaries/talos-linux-sandbox-launcher'
        helper.parent.mkdir(); helper.write_bytes(b'launcher'); helper.chmod(0o755)
        lock = self.root / '.github/sandbox-base.env'
        lock.parent.mkdir()
        valid = (ROOT / '.github/sandbox-base.env').read_text().strip().split('=', 1)[1]
        version = valid.split('@')[0].rsplit(':', 1)[1]
        for image in (valid, valid.split('@')[0], valid.replace(':' + version, ''), valid.replace(':' + version, ':v01.1.0')):
            lock.write_text('CI_SANDBOX_BASE_IMAGE=' + image + '\n')
            with patch.object(ci_run, 'ROOT', self.root), patch.object(ci_run, 'run') as execute:
                if image == valid:
                    ci_run.execute(task, self.root / 'cache')
                    self.assertEqual(execute.call_args_list[0].args, ('docker', 'pull', valid))
                else:
                    with self.assertRaisesRegex(ValueError, 'semantic version and immutable digest'):
                        ci_run.execute(task, self.root / 'cache')
                    execute.assert_not_called()
