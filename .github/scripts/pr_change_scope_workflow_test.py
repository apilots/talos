"""Exercise actual CI commands, failure propagation, required gates and reuse inputs."""
import copy
import json
import os
from pathlib import Path
import subprocess
import tempfile
import tarfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import ci_run
from ci_gate import validate
from ci_impact import ROOT, rust_graph, frontend_graph
from ci_plan import BINARIES, plan


class WorkflowTests(unittest.TestCase):
    def test_cancellation_fences_every_pr_reusable_call_boundary(self):
        import re
        pending, seen = ['.github/workflows/pr-e2e.yml'], set()
        while pending:
            name = pending.pop()
            if name in seen:
                continue
            seen.add(name)
            source = (ROOT / name).read_text()
            pending += [value[2:] for value in re.findall(r'uses: (\./\.github/workflows/[^\s]+)', source)]
            jobs = re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)', source.split('\njobs:\n', 1)[1], re.M | re.S)
            for job, body in jobs:
                condition = re.search(r'^    if: \$\{\{ (.*?) \}\}', body, re.M)
                self.assertIsNotNone(condition, (name, job))
                expression = re.sub(r'!(?!=)', 'not ', condition.group(1).replace('&&', ' and ').replace('||', ' or '))
                # Cancellation must short-circuit even a nested caller whose inputs succeeded.
                self.assertFalse(eval(expression, {'__builtins__': {}, 'cancelled': lambda: True}), (name, job))
        self.assertGreaterEqual(len(seen), 8)

    def test_running_reusable_leaf_jobs_stop_on_workflow_cancellation(self):
        import re
        conditions = re.findall(r'^    if: \$\{\{ (.*?) \}\}',
            (ROOT / '.github/workflows/ci-category.yml').read_text(), re.M)
        self.assertEqual(len(conditions), 2)
        for selected in (False, True):
            inputs = SimpleNamespace(plan=json.dumps(dict(quality=[1] if selected else [],
                scenarios=dict(include=[1] if selected else []))))
            for cancelled in (False, True):
                for condition in conditions:
                    expression = re.sub(r'!(?!=)', 'not ', condition.replace('&&', ' and '))
                    actual = eval(expression, {'__builtins__': {}, 'cancelled': lambda: cancelled,
                        'fromJSON': lambda value: json.loads(value, object_hook=lambda obj: SimpleNamespace(**obj)),
                        'toJSON': json.dumps}, {'inputs': inputs})
                    self.assertEqual(actual, selected and not cancelled)

    def test_partial_retry_consumes_same_run_binary_without_rerunning_build(self):
        import re
        build = (ROOT / '.github/workflows/ci-build.yml').read_text()
        scenario = (ROOT / '.github/workflows/ci-category.yml').read_text()
        names = re.findall(r'name: (ci-(?:binary|tests)-[^\n]+)', build)
        self.assertEqual(len(names), 9)
        for attempt in (2, 3):
            # A failed-job retry consumes stable current-run products from attempt one.
            self.assertTrue(all('run_attempt' not in name for name in names))
            self.assertIn('pattern: ${{ matrix.binary_pattern }}', scenario)
        self.assertEqual(build.count('overwrite: true'), len(names))

    def test_pr_conditions_report_failures_but_allow_obsolete_runs_to_cancel(self):
        import itertools
        import re
        workflow = (ROOT / '.github/workflows/pr-e2e.yml').read_text()
        jobs = dict(re.findall(r'^  ([\w-]+):\n(.*?)(?=^  [\w-]+:|\Z)', workflow, re.M | re.S))
        for job in ('webui', 'desktop', 'tui', 'server', 'frontend-gate', 'rust-gate', 'e2e-gate', 'installer-gate'):
            condition = re.search(r'^    if: \$\{\{ (.*?) \}\}', jobs[job], re.M).group(1)
            expression = condition.replace('&&', ' and ').replace('||', ' or ').replace('!', 'not ')
            for cancelled, build, tests, scope, selected in itertools.product(
                (False, True), ('success', 'skipped', 'failure'), ('success', 'skipped', 'failure'),
                ('success', 'failure'), ('true', 'false')):
                needs = SimpleNamespace(scope=SimpleNamespace(result=scope, outputs=SimpleNamespace(
                    **{name: selected for name in ('webui', 'desktop', 'tui', 'server')})),
                    build=SimpleNamespace(result=build), tests=SimpleNamespace(result=tests))
                actual = eval(expression, {'__builtins__': {}, 'cancelled': lambda: cancelled}, {'needs': needs})
                expected = not cancelled and (job.endswith('-gate') or (
                    scope == 'success' and selected == 'true' and build in ('success', 'skipped')
                    and (job not in ('tui', 'server') or tests in ('success', 'skipped'))))
                self.assertEqual(actual, expected, (job, cancelled, build, tests, scope, selected))

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(dir=ROOT / 'output')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True)
        self.calls = []
        self.run = patch.object(ci_run, 'run', side_effect=lambda *args, **kwargs: self.calls.append((args, kwargs)))
        self.run.start()
        self.addCleanup(self.run.stop)
        self.root_patch = patch.object(ci_run, 'ROOT', self.root)
        self.root_patch.start()
        self.addCleanup(self.root_patch.stop)

    def binary(self, binary):
        path = self.root / 'output/ci-binaries' / BINARIES[binary][1]
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('fixture')
        path.chmod(0o755)

    def test_tui_commands_use_scoped_just_and_real_case(self):
        selected = plan(['talos-rs/apps/tui/src/render/chat_lines.rs'])['categories']['tui']
        ci_run.execute(selected['quality'][0], self.root / 'cache')
        self.assertEqual([args for args, _ in self.calls], [
            ('just', 'fmt-check'), ('just', 'check', '--locked', '-p', 'talos-tui'),
            ('just', 'test', '--locked', '-p', 'talos-tui')])
        self.calls.clear()
        task = selected['scenarios']['include'][0]
        for binary in task['binaries']:
            self.binary(binary)
        ci_run.execute(task, self.root / 'cache')
        args, kwargs = self.calls[-1]
        self.assertEqual(args[-3:], ('just', 'e2e-case', '08_tui/controlled_chat'))
        self.assertEqual(kwargs['env']['TALOS_E2E_CLI_BIN'], str(self.root / 'output/ci-binaries/talos'))

    def test_build_union_is_compiled_once_and_archived(self):
        task = plan(['.github/release-toolchain.env'])['build']
        for b in task['binaries']:
            path = self.root / 'talos-rs/target/debug' / BINARIES[b][1]
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('native-binary')
            path.chmod(0o755)
        task['tests'] = {}  # This regression isolates executable distribution.
        ci_run.execute(task, self.root / 'cache')
        self.assertEqual(len(self.calls), 1)
        args = self.calls[0][0]
        self.assertEqual(args[:3], ('just', 'build', '--locked'))
        for binary in task['binaries']:
            path = self.root / 'cache' / f'binary-{binary}.tar.gz'
            with tarfile.open(path) as archive:
                self.assertEqual(archive.getnames(), [BINARIES[binary][1]])
                self.assertTrue(archive.getmembers()[0].mode & 0o111)

    def test_missing_artifact_cannot_trigger_unplanned_compile(self):
        task = plan(['talos-rs/apps/tui/src/render/chat_lines.rs'])['categories']['tui']['scenarios']['include'][0]
        with self.assertRaisesRegex(ValueError, 'Missing executable'):
            ci_run.execute(task, self.root / 'cache')
        self.assertEqual(self.calls, [])

    def test_frontend_quality_does_not_build_web_twice_for_desktop(self):
        task = plan(['talos-app/desktop/src/main.ts'])['categories']['desktop']['quality'][0]
        ci_run.execute(task, self.root / 'cache')
        self.assertIn(('pnpm', '--dir', 'talos-app', '--filter', 'talos-desktop', 'build:main'), [c[0] for c in self.calls])
        self.assertFalse(any(c[0][-1] == 'build' for c in self.calls))

    def test_gate_rejects_selected_failures_and_unexpected_skips(self):
        selected = plan(['talos-rs/apps/tui/src/render/chat_lines.rs'])
        needs = {name: {'result': 'skipped'} for name in ('scope','build','tests','webui','desktop','server','tui','harness','installer')}
        for name in ('scope', 'build', 'tests', 'tui'):
            needs[name]['result'] = 'success'
        for mode in ('frontend','rust','e2e','installer'):
            validate(mode, selected, needs)
        for state in ('failure', 'cancelled', 'skipped'):
            bad = copy.deepcopy(needs)
            bad['tui']['result'] = state
            with self.assertRaises(ValueError):
                validate('e2e', selected, bad)
        bad = copy.deepcopy(needs)
        bad['scope']['result'] = 'failure'
        with self.assertRaises(ValueError):
            validate('frontend', selected, bad)
        bad = copy.deepcopy(needs)
        bad['desktop']['result'] = 'success'
        with self.assertRaises(ValueError):
            validate('frontend', selected, bad)

    def test_success_records_require_matching_inputs_and_intact_build(self):
        task = plan(['talos-rs/apps/tui/src/render/chat_lines.rs'])['build']
        directory = self.root / 'cache'
        directory.mkdir()
        (directory / 'binary-cli.tar.gz').write_bytes(b'archive')
        self.assertFalse(ci_run.cached(directory, 'key', task))
        ci_run.record(directory, 'key', task)
        self.assertTrue(ci_run.cached(directory, 'key', task))
        self.assertFalse(ci_run.cached(directory, 'new-key', task))
        self.assertFalse(ci_run.cached(directory, 'key', {**task, 'name': 'other'}))
        (directory / 'binary-cli.tar.gz').write_bytes(b'corrupted')
        self.assertFalse(ci_run.cached(directory, 'key', task))
        record = json.loads((directory / 'success.json').read_text())
        record['conclusion'] = 'failure'
        (directory / 'success.json').write_text(json.dumps(record))
        self.assertFalse(ci_run.cached(directory, 'key', task))

    def test_fingerprint_tracks_merge_tree_inputs_not_last_commit(self):
        # Copy graph manifests, then exercise source/dependency/command changes.
        manifests = ['talos-rs/Cargo.toml', 'talos-app/package.json']
        manifests += [path + '/Cargo.toml' for path in rust_graph().roots.values()]
        manifests += [path + '/package.json' for path in frontend_graph().roots.values()]
        for name in manifests:
            destination = self.root / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes((ROOT / name).read_bytes())
        def write(path, value):
            file = self.root / path
            file.parent.mkdir(parents=True, exist_ok=True)
            file.write_text(value)
        task = dict(name='server-build',kind='build',binaries=['server'],rust=[],frontend=[])
        write('talos-rs/bin/talos-server/src/main.rs', 'first')
        source = self.root / 'talos-rs/bin/talos-server/src/main.rs'
        source.chmod(0o644)
        first = ci_run.fingerprint(task, self.root)
        source.chmod(0o664)
        self.assertEqual(ci_run.fingerprint(task, self.root), first)
        source.chmod(0o744)
        self.assertNotEqual(ci_run.fingerprint(task, self.root), first)
        source.chmod(0o644)
        write('talos-app/web/src/presentation.css', 'unrelated')
        self.assertEqual(ci_run.fingerprint(task, self.root), first)
        write('.github/ISSUE_TEMPLATE/agent-task.yml', 'unrelated-template')
        write('conf/migrations/README.md', 'unrelated-documentation')
        self.assertEqual(ci_run.fingerprint(task, self.root), first)
        write('talos-rs/bin/talos-server/src/main.rs', 'updated-merge-tree')
        second = ci_run.fingerprint(task, self.root)
        self.assertNotEqual(second, first)
        write('talos-rs/crates/orchestration/service/src/lib.rs', 'dependency')
        self.assertNotEqual(ci_run.fingerprint(task, self.root), second)
        third = ci_run.fingerprint(task, self.root)
        write('.github/workflows/ci-build.yml', 'changed-command')
        self.assertNotEqual(ci_run.fingerprint(task, self.root), third)
        self.assertNotEqual(ci_run.fingerprint({**task,'binaries':['cli']}, self.root), ci_run.fingerprint(task, self.root))

    def test_failed_or_cancelled_prior_workflow_and_manual_rerun_cannot_reuse_success(self):
        import io
        task = plan(['talos-rs/apps/tui/src/render/chat_lines.rs'])['categories']['tui']['quality'][0]
        directory = self.root / 'cache'
        ci_run.record(directory, 'key', task)
        row = json.loads((directory / 'success.json').read_text())
        row['run'] = '123'
        (directory / 'success.json').write_text(json.dumps(row))
        with patch.dict(os.environ, GITHUB_ACTIONS='true', GH_TOKEN='controlled-test-token', GITHUB_REPOSITORY='owner/repo'):
            for status, conclusion, expected in [('completed','success',True), ('completed','failure',False),
                                                  ('completed','cancelled',False), ('in_progress',None,False)]:
                response = io.BytesIO(json.dumps(dict(status=status,conclusion=conclusion)).encode())
                with patch.object(ci_run.urllib.request,'urlopen',return_value=response):
                    self.assertEqual(ci_run.reusable(directory,'key',task),expected)
            with patch.dict(os.environ, CI_FORCE_RECHECK='true'):
                self.assertFalse(ci_run.reusable(directory,'key',task))
            with patch.object(ci_run.urllib.request,'urlopen',side_effect=OSError('unavailable')):
                self.assertFalse(ci_run.reusable(directory,'key',task))

    def test_native_mcp_and_harness_roots_include_external_resolution_and_fixture_inputs(self):
        from ci_plan import task_roots
        native = task_roots(dict(kind='mcp',rust=[],frontend=[],binaries=[]))
        self.assertIn('talos-rs', native)  # Includes root Cargo.lock and .cargo.
        fixture = task_roots(dict(kind='harness',rust=[],frontend=[],binaries=[]))
        self.assertTrue({'talos-e2e','talos-app','talos-rs'} <= fixture)

    def test_harness_uses_existing_owner_import_path(self):
        ci_run.execute(dict(kind='harness'), self.root / 'cache')
        self.assertEqual(len(self.calls), 3)
        for _, kwargs in self.calls:
            self.assertEqual(kwargs['env']['PYTHONPATH'].split(os.pathsep)[0],
                             str(self.root / 'talos-e2e/scripts'))

    def test_sandbox_scenarios_include_native_test_toolchain_and_dependencies(self):
        from ci_plan import scenario_task, task_roots
        sandbox = scenario_task('server', 'group:14_sandbox')
        self.assertTrue(sandbox['rust_tests'])
        self.assertEqual(sandbox['rust'], ['talos-tools'])
        self.assertIn('talos-rs/crates/action/tools', task_roots(sandbox))
        tui = scenario_task('tui', 'case:08_tui/controlled_chat')
        self.assertFalse(tui['rust_tests'])
        self.assertFalse(tui['rust'])

    def test_environment_and_pr_isolate_success_keys(self):
        task = plan(['talos-rs/apps/tui/src/render/chat_lines.rs'])['build']
        with patch.dict(os.environ, CI_PR_NUMBER='1', ImageVersion='first'):
            first, _ = ci_run.cache_key(task)
            with patch.dict(os.environ, CI_PR_NUMBER='2'):
                self.assertNotEqual(ci_run.cache_key(task)[0], first)
            with patch.dict(os.environ, ImageVersion='updated'):
                self.assertNotEqual(ci_run.cache_key(task)[0], first)
            with patch.dict(os.environ, RUSTFLAGS='-C target-cpu=x86-64'):
                self.assertNotEqual(ci_run.cache_key(task)[0], first)

    def test_command_failure_never_writes_success(self):
        task = plan(['talos-rs/apps/tui/src/render/chat_lines.rs'])['categories']['tui']['quality'][0]
        with patch.dict(os.environ, CI_TASK=json.dumps(task), CI_CACHE_KEY='test-key', CI_CACHE_DIR=str(self.root/'failed')):
            with patch.object(ci_run, 'run', side_effect=subprocess.CalledProcessError(1, 'just')):
                with patch.object(ci_run.sys, 'argv', ['ci_run.py','run']):
                    with self.assertRaises(subprocess.CalledProcessError):
                        ci_run.main()
        self.assertFalse((self.root/'failed/success.json').exists())


if __name__ == '__main__':
    unittest.main()
