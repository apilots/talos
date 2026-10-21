"""Resource cleanup regressions use real local processes and a controlled Docker CLI."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1] / 'scripts'
sys.path.insert(0, str(SCRIPTS))
import lifecycle


class LifecycleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.env = dict(os.environ, TALOS_E2E_RESOURCE_ROOT=str(self.root/'run'),
                        TALOS_E2E_RUN_ID='owned-test', TALOS_E2E_CASE_ID='20_fixture/case')
        self.patch = patch.dict(os.environ, self.env)
        self.patch.start()
        self.addCleanup(self.patch.stop)
        self.children = []
        self.addCleanup(self.stop_children)
        self.addCleanup(self.remove_socket_directories)

    def remove_socket_directories(self):
        ledger = lifecycle.root()/'resources.jsonl'
        if ledger.exists():
            for line in ledger.read_text().splitlines():
                record = json.loads(line)
                if record['kind'] == 'directory':
                    lifecycle.remove_directory(record)

    def stop_children(self):
        for p in self.children:
            if p.poll() is None:
                os.killpg(p.pid, signal.SIGKILL)
            p.wait()

    def test_cleanup_terminates_only_registered_groups_and_is_idempotent(self):
        for _ in range(2):
            self.children.append(subprocess.Popen([sys.executable, '-c', 'import time;time.sleep(60)'], start_new_session=True))
        lifecycle.own_process(self.children[0])
        self.assertEqual(lifecycle.cleanup('20_fixture/case'), [])
        self.assertIsNotNone(self.children[0].poll())
        self.assertIsNone(self.children[1].poll())
        self.assertEqual(lifecycle.cleanup(), [])

    def test_process_identity_change_refuses_cleanup(self):
        self.children.append(subprocess.Popen(['sleep','60'], start_new_session=True))
        lifecycle.own_process(self.children[0])
        ledger=lifecycle.root()/'resources.jsonl'
        row=json.loads(ledger.read_text());row['started']='not this process'
        ledger.write_text(json.dumps(row)+'\n')
        self.assertIn('reused process ID', lifecycle.cleanup()[0])
        self.assertIsNone(self.children[0].poll())

    def run_case(self, source, status=0, interrupt=None):
        self.script=self.root/'scenario.sh'
        self.script.write_text(source + f'\nexit {status}\n')
        self.result=subprocess.Popen([sys.executable,str(SCRIPTS/'case_run.py'),str(self.script)],env=self.env,
                                     stdout=subprocess.PIPE,stderr=subprocess.PIPE,text=True)
        if interrupt:
            deadline=time.monotonic()+5
            while not (self.root/'ready').exists():
                if time.monotonic()>deadline: self.result.kill();self.fail('case did not start')
                time.sleep(.02)
            self.result.send_signal(interrupt)
        stdout,stderr=self.result.communicate(timeout=12)
        return self.result.returncode,stdout+stderr

    def test_case_success_and_failure_remove_private_temp_and_orphaned_child(self):
        for status in [0,7]:
            with self.subTest(status=status):
                code,log=self.run_case('''touch "$TMPDIR/intermediate"
python3 -c 'import subprocess;from pathlib import Path;p=subprocess.Popen(["sleep","60"],start_new_session=True);Path("'''+str(self.root/'pid')+'''").write_text(str(p.pid))'
''',status)
                self.assertEqual(code,status,log)
                self.assertFalse((lifecycle.root()/'cases/20_fixture/case').exists())
                pid=int((self.root/'pid').read_text())
                check=subprocess.run(['ps','-p',str(pid),'-o','stat='],capture_output=True,text=True)
                self.assertTrue(check.returncode!=0 or check.stdout.strip().startswith('Z'))

    def test_interrupt_retires_case_children_and_preserves_unrelated_files(self):
        keep=self.root/'unrelated';keep.write_text('keep')
        for sig in [signal.SIGINT,signal.SIGTERM]:
            (self.root/'ready').unlink(missing_ok=True)
            code,log=self.run_case(f'touch "{self.root}/ready"\nsleep 60',interrupt=sig)
            self.assertEqual(code,128+sig,log)
            self.assertFalse((lifecycle.root()/'cases/20_fixture/case').exists())
            self.assertEqual(keep.read_text(),'keep')

    def test_docker_failure_is_reported_and_other_owner_is_never_removed(self):
        lifecycle.register('volume','owned-volume')
        calls=[]
        def command(args,**kwargs):
            calls.append(args)
            if args[1:3]==['volume','inspect']:
                return subprocess.CompletedProcess(args,0,json.dumps([{'Labels':{lifecycle.LABEL:'other-run'}}]),'')
            raise AssertionError(args)
        with patch.object(lifecycle.subprocess,'run',side_effect=command):
            self.assertIn('another run',lifecycle.cleanup()[0])
        self.assertFalse(any('rm' in c for c in calls))
        def unavailable(args,**kwargs):
            if args[1]=='info':raise subprocess.CalledProcessError(1,args)
            return subprocess.CompletedProcess(args,1,'','daemon unavailable')
        with patch.object(lifecycle.subprocess,'run',side_effect=unavailable):
            self.assertTrue(lifecycle.cleanup())
        self.assertTrue((lifecycle.root()/'resources.jsonl').read_text())

    def test_case_and_run_scopes_remove_only_owned_resources_in_dependency_order(self):
        for kind in ['image', 'network', 'volume', 'container']:
            lifecycle.register(kind, 'owned-' + kind, 'run' if kind == 'image' else None)
        removed = []
        def command(args, **kwargs):
            if args[2] == 'inspect':
                labels = {lifecycle.LABEL: 'owned-test'}
                doc = {'Config': {'Labels': labels}} if args[1] in ('image', 'container') else {'Labels': labels}
                return subprocess.CompletedProcess(args, 0, json.dumps([doc]), '')
            removed.append(args)
            return subprocess.CompletedProcess(args, 0, '', '')
        with patch.object(lifecycle.subprocess, 'run', side_effect=command):
            self.assertEqual(lifecycle.cleanup('20_fixture/case'), [])
            self.assertEqual([args[-1] for args in removed], ['owned-container', 'owned-volume', 'owned-network'])
            self.assertEqual(json.loads((lifecycle.root()/'resources.jsonl').read_text())['kind'], 'image')
            self.assertEqual(lifecycle.cleanup(), [])
            self.assertEqual(removed[-1], ['docker', 'image', 'rm', 'owned-image'])

    def test_case_cleanup_error_fails_success_and_preserves_original_failure(self):
        binary = self.root / 'bin'
        binary.mkdir()
        docker = binary / 'docker'
        docker.write_text('#!/bin/sh\nexit 1\n')
        docker.chmod(0o755)
        self.env['PATH'] = str(binary) + os.pathsep + self.env['PATH']
        for status in [0, 7]:
            with self.subTest(status=status):
                code, log = self.run_case(f'python3 "{SCRIPTS}/lifecycle.py" register volume owned-volume', status)
                self.assertEqual(code, status or 1, log)
                self.assertIn('Cleanup failed for volume', log)
                self.assertTrue((lifecycle.root()/'cases/20_fixture/case').is_dir())
                import shutil
                shutil.rmtree(lifecycle.root()/'cases')

    def test_case_id_cannot_escape_run_root(self):
        self.env['TALOS_E2E_CASE_ID'] = '../outside'
        code, log = self.run_case('exit 0')
        self.assertNotEqual(code, 0)
        self.assertIn('relative catalog path', log)
        self.assertFalse((self.root/'outside').exists())

    def test_tmux_cleanup_targets_only_the_case_private_socket(self):
        binary = self.root / 'bin'
        binary.mkdir()
        command = binary / 'tmux'
        calls = self.root / 'tmux-calls'
        command.write_text('#!/bin/sh\nprintf "%s\\n" "$*" >> "' + str(calls) + '"\n')
        command.chmod(0o755)
        self.env['PATH'] = str(binary) + os.pathsep + self.env['PATH']
        external = self.root / 'user-tmux'
        external.write_text('untouched')
        self.env['TMUX'] = str(external) + ',1,0'
        code, log = self.run_case('''set -e
test -z "${TMUX:-}"
printf "%s" "$TMUX_TMPDIR" > "'''+str(self.root/'socket-root')+'''"
python3 -c 'import os,socket;from pathlib import Path;p=Path(os.environ["TMUX_TMPDIR"])/"tmux-test";p.mkdir();s=socket.socket(socket.AF_UNIX);s.bind(str(p/"default"))'
''')
        self.assertEqual(code, 0, log)
        private = Path((self.root/'socket-root').read_text())
        socket = private/'tmux-test/default'
        self.assertEqual(calls.read_text(), f'-S {socket} kill-server\n')
        self.assertFalse(private.exists())
        self.assertEqual(external.read_text(), 'untouched')

    def test_short_socket_directory_refuses_another_runs_owner_marker(self):
        directory = lifecycle.socket_directory()
        (directory/'.e2e-owner').write_text('other-run')
        try:
            self.assertIn('another run', lifecycle.cleanup()[0])
            self.assertTrue(directory.exists())
        finally:
            (directory/'.e2e-owner').write_text('owned-test')
            self.assertEqual(lifecycle.cleanup(), [])

    def test_run_image_is_reused_and_external_image_not_registered(self):
        images={};builds=[]
        def command(args,**kwargs):
            if args[1:3]==['image','inspect']:
                name=args[-1]
                return subprocess.CompletedProcess(args,0 if name in images or name=='shared:fixed' else 1,
                                                   json.dumps([{'Config':{'Labels':{lifecycle.LABEL:'owned-test'}}}]),'')
            if args[1]=='build':
                name=args[args.index('-t')+1];images[name]=True;builds.append(args)
                return subprocess.CompletedProcess(args,0)
            raise AssertionError(args)
        with patch.object(lifecycle.subprocess,'run',side_effect=command):
            self.assertEqual(lifecycle.build_image('worker','shared:fixed',[]),'shared:fixed')
            self.assertFalse((lifecycle.root()/'resources.jsonl').exists())
            first=lifecycle.build_image('worker','',['.'])
            self.assertEqual(lifecycle.build_image('worker','',['.']),first)
        self.assertEqual(len(builds),1)
        self.assertEqual(json.loads((lifecycle.root()/'resources.jsonl').read_text())['scope'],'run')


if __name__=='__main__':unittest.main()
