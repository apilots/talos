from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from report import load_attempts
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
