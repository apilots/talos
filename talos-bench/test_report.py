from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from report import load_attempts, render
from summarize import write_json


class ReportPinTest(unittest.TestCase):
    def test_report_matches_dataset_and_original_task_pin(self):
        dataset = {"name": "suite", "version": "1.0", "source": "upstream/suite", "digest": "new"}
        registry = {"task": {"digest": "original"}}
        with TemporaryDirectory() as directory:
            runs = Path(directory)
            for name, dataset_ref, digest in (
                ("old-dataset", "upstream/suite@old", "original"),
                ("old-task", "upstream/suite@new", "other"),
                ("native", "upstream/suite@new", "native-digest"),
            ):
                run = runs / name
                (run / "provenance").mkdir(parents=True)
                write_json(run / "summary.json", {
                    "suite": "suite", "suiteVersion": "1.0", "mode": "real", "datasetRef": dataset_ref,
                    "trials": [{"task": "task", "model": "model", "taskDigest": digest}],
                })
                if name == "native":
                    write_json(run / "provenance/task.json", {
                        "taskRef": "upstream/task@original", "localTaskDigest": "native-digest",
                    })
            (summary, _), = load_attempts(dataset, runs, registry, "model")["task"]
            self.assertEqual(summary.parent.name, "native")

    def test_light_report_keeps_incomplete_attempt_separate_from_medium_score(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            runs = root / "runs"
            registry = root / "registry.json"
            output = root / "report.html"
            write_json(registry, [{"name": "suite", "version": "1.0", "tasks": [{"name": "task"}]}])
            for effort, score in (("medium", 1), ("light", None)):
                run = runs / f"run-{effort}"
                run.mkdir(parents=True)
                write_json(run / "trajectory.json", {})
                write_json(run / "summary.json", {
                    "suite": "suite", "suiteVersion": "1.0", "mode": "real",
                    "trials": [{
                        "task": "task", "model": "openai:gpt-6.1-terra", "reasoningEffort": effort,
                        "score": score, "cliExitCode": 0, "modelCallsObserved": 1,
                        "artifacts": {"trajectory": "trajectory.json"},
                    }],
                })

            render(output, registry, runs, "openai:gpt-6.1-terra", {}, [], reasoning_effort="light")
            report = output.read_text()
            self.assertIn("暂无有效评分", report)
            self.assertIn("run-light", report)
            self.assertNotIn("run-medium", report)
