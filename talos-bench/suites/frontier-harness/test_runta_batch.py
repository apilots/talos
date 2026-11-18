import importlib.util
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("runta_batch", Path(__file__).with_name("runta_batch.py"))
batch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(batch)


class BatchTest(unittest.TestCase):
    def test_resuming_the_controller_skips_started_cases(self):
        with TemporaryDirectory() as temp:
            root = Path(temp)
            task = "terminal-bench/regex-log"
            args = ["runta_batch.py", str(root), "--task", task]
            with patch.object(sys, "argv", args), patch.object(batch, "config", return_value={}), \
                 patch.object(batch.subprocess, "run", return_value=subprocess.CompletedProcess([], 1)) as run:
                batch.main()
                batch.main()
            run.assert_called_once()
            state = json.loads((batch.case_dir(root, task) / "state.json").read_text())
            self.assertEqual(state["exit_code"], 1)

    def test_only_launcher_verified_archives_are_recorded(self):
        for verified in (False, True):
            with self.subTest(verified=verified), TemporaryDirectory() as temp:
                root = Path(temp)
                output = root / "result"
                output.mkdir()
                (output / "original-job.tar.gz").write_bytes(b"downloaded job")
                (output / "summary.json").write_text('{"complete": true, "scoredTrials": 1, "meanScore": 1.0}')
                if verified:
                    (output / "original-job.sha256").write_text("a" * 64 + "\n")

                def failed_launch(*args, **kwargs):
                    kwargs["stdout"].write(f"Talos benchmark run: {output}\nRunta runtime: retained-case\n")
                    return subprocess.CompletedProcess([], 1)

                with patch.object(batch.subprocess, "run", side_effect=failed_launch):
                    batch.run_case(root, "terminal-bench/example")
                state = json.loads((batch.case_dir(root, "terminal-bench/example") / "state.json").read_text())
                self.assertEqual(state["runtime"], "retained-case")
                self.assertEqual(state["exit_code"], 1)
                if verified:
                    self.assertEqual(state["archive_sha256"], "a" * 64)
                    self.assertEqual(state["score"], 1.0)
                else:
                    self.assertNotIn("archive_sha256", state)
                    self.assertNotIn("score", state)


if __name__ == "__main__":
    unittest.main()
