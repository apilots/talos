import importlib.util
import json
import os
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

    def test_resume_pins_forwarded_agent_overrides(self):
        first, pending = "terminal-bench/regex-log", "terminal-bench/embedding-drift-monitor"
        overrides = "TALOS_BENCH_COLLABORATION_TOOLS=0"
        settings = {
            "TALOS_BENCH_RUNTA_CHECKPOINT": "test-checkpoint",
            "TALOS_BENCH_RUNTA_SECRET": "test-secret-name",
            "TALOS_BENCH_MODEL": "openai:test-model",
            "TALOS_BENCH_REASONING_EFFORT": "medium",
            "TALOS_BENCH_AGENT_ENV": overrides,
        }
        with TemporaryDirectory() as temp:
            root = Path(temp)
            args = ["runta_batch.py", str(root), "--task", first, "--task", pending, "--limit", "1"]
            with patch.object(sys, "argv", args), patch.dict(os.environ, settings), \
                 patch.object(batch, "run_case") as launch:
                batch.main()
                launch.assert_called_once_with(root, first)
                batch.case_dir(root, first).mkdir(parents=True)
                manifest = (root / "manifest.json").read_text()

                os.environ["TALOS_BENCH_AGENT_ENV"] = "TALOS_BENCH_COLLABORATION_TOOLS=1"
                with self.assertRaisesRegex(SystemExit, "Batch manifest differs"):
                    batch.main()
                launch.assert_called_once()
                self.assertEqual((root / "manifest.json").read_text(), manifest)
                self.assertNotIn(overrides, manifest)

                os.environ["TALOS_BENCH_AGENT_ENV"] = overrides
                batch.main()
                self.assertEqual(launch.call_count, 2)
                launch.assert_called_with(root, pending)

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
