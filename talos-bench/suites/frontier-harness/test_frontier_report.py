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
                "trial": "rstan-to-pystan__trial",
                "taskDigest": "sha256:adapted",
                "score": 1,
                "cliExitCode": 0,
                "artifacts": {"atif": "trajectory.json", "harborResult": "result.json"},
            }
            result = {"task_name": task_name, "trial_name": trial["trial"],
                      "verifier_result": {"rewards": {"reward": 1}}}
            provenance = {"taskRef": f"{task_name}@{original_digest}", "localTaskDigest": trial["taskDigest"]}
            summary = {"datasetRef": dataset_ref, "platform": "linux/arm64", "mode": "real", "complete": True,
                       "trials": [trial]}
            (retry / "result.json").write_text(json.dumps(result))
            trace = '{"schema_version":"ATIF-v1.8","steps":[{"step_id":1}]}'
            (retry / "trajectory.json").write_text(trace)
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
            original_trial = {**trial, "taskDigest": original_digest, "score": 0}
            original_summary = {**summary, "trials": [original_trial]}
            original_result = {**result, "verifier_result": {"rewards": {"reward": 0}}}
            (case / "summary.json").write_text(json.dumps(original_summary))
            (case / "result.json").write_text(json.dumps(original_result))
            (case / "trajectory.json").write_text(trace)
            retained, _ = render(run, inventory, destination, [path])
            self.assertEqual(retained["tasks"][0]["score"], 0)
            self.assertNotIn("supplemental", retained["tasks"][0])

            for filename in ("result.json", "trajectory.json"):
                with self.subTest(missing=filename):
                    artifact = case / filename
                    saved = artifact.read_text()
                    artifact.unlink()
                    unverified, _ = render(run, inventory, destination, [])
                    self.assertEqual(unverified["officialScored"], 0)
                    self.assertEqual(unverified["tasks"][0]["status"], "missing")
                    artifact.write_text(saved)

            original_trial["taskDigest"] = "sha256:stale"
            (case / "summary.json").write_text(json.dumps(original_summary))
            unverified, _ = render(run, inventory, destination, [])
            self.assertEqual(unverified["officialScored"], 0)
            recovered, _ = render(run, inventory, destination, [path])
            self.assertTrue(recovered["tasks"][0]["supplemental"])
