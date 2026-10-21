"""Exercise common binary preparation without compiling or replacing user artifacts."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

COMMON = Path(__file__).resolve().parents[1] / 'scripts/common.sh'


class BinaryReuseTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup)
        self.root=Path(self.temp.name)
        (self.root/'bin').mkdir()
        cargo=self.root/'bin/cargo'
        cargo.write_text('''#!/bin/bash
printf '%s\\n' "$*" >> "$CALLS"
name="";profile=debug
while [[ $# -gt 0 ]]; do
  case "$1" in --bin) name="$2";shift;; --release) profile=release;; esac
  shift
done
mkdir -p "target/$profile"
printf '#!/bin/sh\\nexit 0\\n' > "target/$profile/$name"
chmod +x "target/$profile/$name"
''');cargo.chmod(0o755)
        self.env=dict(os.environ,PATH=str(self.root/'bin')+os.pathsep+os.environ['PATH'],CALLS=str(self.root/'calls'))

    def run_shell(self,source,**env):
        return subprocess.run(['bash','-c',f'set -euo pipefail\nsource "{COMMON}"\nTALOS_RS_DIR="{self.root}"\n'+source],
                              env={**self.env,**env},capture_output=True,text=True)

    def executable(self,path):
        p=self.root/path;p.parent.mkdir(parents=True,exist_ok=True)
        p.write_text('#!/bin/sh\nexit 0\n');p.chmod(0o755);return str(p)

    def test_supplied_runtime_and_neighbor_sandbox_do_not_build_debug_or_overwrite_files(self):
        server=self.executable('target/release/talos-server')
        sandbox=self.executable('target/release/talos-linux-sandbox-launcher')
        r=self.run_shell('build_server_binary\nbuild_sandbox_binary\nprintf "%s\\n" "$TALOS_E2E_SERVER_BIN" "$TALOS_E2E_SANDBOX_BIN"',TALOS_E2E_SERVER_BIN=server)
        self.assertEqual(r.returncode,0,r.stderr)
        self.assertEqual(r.stdout.splitlines(),[server,sandbox])
        self.assertFalse((self.root/'calls').exists())
        self.assertFalse((self.root/'target/debug').exists())

    def test_invalid_explicit_path_fails_without_fallback(self):
        r=self.run_shell('build_server_binary',TALOS_E2E_SERVER_BIN=str(self.root/'missing'))
        self.assertNotEqual(r.returncode,0)
        self.assertFalse((self.root/'calls').exists())

    def test_repeated_binary_preparation_builds_each_owner_once(self):
        r=self.run_shell('build_server_binary\nbuild_server_binary\nbuild_worker_binary\nbuild_worker_binary\nbuild_app_server_binaries\nbuild_app_server_binaries')
        self.assertEqual(r.returncode,0,r.stderr)
        calls=(self.root/'calls').read_text().splitlines()
        self.assertEqual(len(calls),4,calls)
        for owner in ['talos-server','talos-worker-bin','app-server','talos-app-server-test-client']:
            self.assertEqual(sum(f'-p {owner} --bin ' in call for call in calls),1)

    def test_missing_neighbor_builds_only_matching_profile_sandbox(self):
        server=self.executable('target/release/talos-server')
        r=self.run_shell('build_server_binary\nbuild_sandbox_binary',TALOS_E2E_SERVER_BIN=server)
        self.assertEqual(r.returncode,0,r.stderr)
        calls=(self.root/'calls').read_text().splitlines()
        self.assertEqual(len(calls),1)
        self.assertIn('-p talos-linux-sandbox --bin talos-linux-sandbox-launcher --release',calls[0])


if __name__=='__main__':unittest.main()
