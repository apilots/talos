"""Verify CI package installation excludes unrelated repositories and retains failures."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class AptInstallTests(unittest.TestCase):
    def test_repository_scope_and_install_failure_propagation(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            scripts = {
                'sudo': 'exec "$@"',
                'getent': 'exit 0',
                'tee': 'cat >/dev/null',
                'apt-get': '''
printf '%s\\n' "$*" >> "$APT_TEST_LOG"
case " $* " in
  *" -o Dir::Etc::sourcelist=/etc/apt/sources.list.d/ubuntu.sources -o Dir::Etc::sourceparts=- "*) ;;
  *) echo 'Unrelated repository: 403 Forbidden' >&2; exit 100 ;;
esac
case " $* " in
  *" update "*) exit "${APT_TEST_UPDATE_EXIT:-0}" ;;
  *" install "*) exit "${APT_TEST_INSTALL_EXIT:-0}" ;;
esac
exit 1
''',
            }
            for name, body in scripts.items():
                script = root / name
                script.write_text('#!/bin/sh\n' + body + '\n')
                script.chmod(0o755)
            for update_exit, install_exit, commands in ((0, 0, 2), (100, 0, 2), (0, 100, 4)):
                with self.subTest(update_exit=update_exit, install_exit=install_exit):
                    log = root / 'commands.log'
                    log.unlink(missing_ok=True)
                    result = subprocess.run(['bash', str(ROOT / '.github/scripts/ci_install_apt.sh'), 'ripgrep'],
                        env={**os.environ, 'PATH': f'{root}{os.pathsep}{os.environ["PATH"]}',
                             'APT_TEST_LOG': str(log), 'APT_TEST_UPDATE_EXIT': str(update_exit),
                             'APT_TEST_INSTALL_EXIT': str(install_exit)}, capture_output=True, text=True)
                    self.assertEqual(result.returncode, update_exit or install_exit)
                    recorded = log.read_text().splitlines()
                    self.assertEqual(len(recorded), commands)
                    if update_exit:
                        self.assertTrue(all(' install ' not in f' {line} ' for line in recorded))


if __name__ == '__main__':
    unittest.main()
