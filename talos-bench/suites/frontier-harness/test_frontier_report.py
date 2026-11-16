import json
from pathlib import Path
from runpy import run_path
from tempfile import TemporaryDirectory
import unittest

render = run_path(str(Path(__file__).with_name("report.py")))["render"]


class FrontierReportTest(unittest.TestCase):
    def test_native_exclusions_and_valid_retries_have_separate_evidence(self):
        registry, = json.loads(Path(__file__).with_name("registry.json").read_text())
        dataset_ref = f"{registry['source']}@{registry['digest']}"
        exclusion_name = "terminal-bench/install-windows-3.11"
        exclusion = registry["native_exclusions"][exclusion_name]
        task_name = "terminal-bench/rstan-to-pystan"
        original_digest = "sha256:original"
        inventory = {
            "datasetRef": dataset_ref,
            "platform": "linux/arm64",
            "tasks": [
                {"name": task_name, "digest": original_digest, "status": "needsNativeBuild", "reasons": []},
                {"name": exclusion_name, "digest": exclusion["digest"], "status": "blocked", "reasons": []},
            ],
        }

        with TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            run, retry, destination = (root / folder for folder in ("original", "retry", "report"))
            case = run / "cases/rstan-to-pystan"
            case.mkdir(parents=True)
            retry.mkdir()
            outcome = {
                "name": task_name,
                "status": "modelFailure",
                "reason": "AgentTimeoutError",
                "reward": 0,
                "officialReward": 1,
            }
            (case / "outcome.json").write_text(json.dumps(outcome))
            original, _ = render(run, inventory, destination, [])
            self.assertEqual(original["officialScored"], 0)
            self.assertEqual(original["tasks"][0]["localScore"], 0)

            trial = {
                "task": task_name,
                "taskDigest": "sha256:adapted",
                "score": 1,
                "cliExitCode": 0,
                "artifacts": {"atif": "trajectory.json", "harborResult": "result.json"},
            }
            result = {"task_name": task_name, "verifier_result": {"rewards": {"reward": 1}}}
            provenance = {"taskRef": f"{task_name}@{original_digest}", "localTaskDigest": trial["taskDigest"]}
            summary = {"datasetRef": dataset_ref, "platform": "linux/arm64", "mode": "real", "complete": True,
                       "trials": [trial]}
            (retry / "result.json").write_text(json.dumps(result))
            (retry / "trajectory.json").write_text('{"schema_version":"ATIF-v1.8"}')
            path = retry / "summary.json"
            path.write_text(json.dumps(summary))
            with self.assertRaisesRegex(ValueError, "task pin mismatch"):
                render(run, inventory, destination, [path])
            (retry / "provenance.json").write_text(json.dumps(provenance))

            reconciled, document = render(run, inventory, destination, [path])
            recovered, excluded = reconciled["tasks"]
            self.assertEqual((reconciled["officialScored"], reconciled["excluded"]), (1, 1))
            self.assertEqual(recovered["verifierResultPath"], "../retry/result.json")
            self.assertIn("../original/cases/rstan-to-pystan/outcome.json", document)
            self.assertEqual(excluded["exclusion"], "guestEmulation")
            self.assertIsNone(excluded["score"])
            self.assertIn(exclusion["reason"], excluded["reasons"])

            outcome.update(status="scored", reward=0, officialReward=0, reason=None)
            (case / "outcome.json").write_text(json.dumps(outcome))
            retained, _ = render(run, inventory, destination, [path])
            self.assertEqual(retained["tasks"][0]["score"], 0)
            self.assertNotIn("supplemental", retained["tasks"][0])
