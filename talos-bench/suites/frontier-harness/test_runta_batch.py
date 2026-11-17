import importlib.util
import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("runta_batch", Path(__file__).with_name("runta_batch.py"))
batch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(batch)


class BatchTest(unittest.TestCase):
    def test_existing_case_is_never_relaunched(self):
        with TemporaryDirectory() as temp:
            root = Path(temp)
            batch.case_dir(root, "terminal-bench/example").mkdir(parents=True)
            with patch.object(batch.subprocess, "run") as run:
                self.assertEqual(batch.run_case(root, "terminal-bench/example")[1], "already-started")
                run.assert_not_called()

    def test_persists_failure_and_runtime_without_retry(self):
        with TemporaryDirectory() as temp:
            root = Path(temp)
            def fake_run(*args, **kwargs):
                kwargs["stdout"].write("Talos benchmark run: /missing/result\nRunta runtime: retained-case\n")
                return type("Result", (), {"returncode": 1})()
            with patch.object(batch.subprocess, "run", side_effect=fake_run):
                batch.run_case(root, "terminal-bench/example")
            state = json.loads((batch.case_dir(root, "terminal-bench/example") / "state.json").read_text())
            self.assertEqual(state["runtime"], "retained-case")
            self.assertEqual(state["exit_code"], 1)
            self.assertNotIn("score", state)


if __name__ == "__main__":
    unittest.main()
