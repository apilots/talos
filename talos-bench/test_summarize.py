import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from summarize import read_json, summarize, write_json


class CompletionTest(unittest.TestCase):
    def test_zero_reward_requires_the_declared_evidence(self):
        with TemporaryDirectory() as directory:
            run = Path(directory)
            job = run / "test-suite"
            trial = job / "test-task__trial"
            agent = trial / "agent"
            artifacts = trial / "artifacts"
            agent.mkdir(parents=True)
            artifacts.mkdir()
            registry = run / "registry.json"
            write_json(registry, [{"version": "1.0", "tasks": [
                {"name": "test-task", "submission_paths": ["/app/answer.txt"]},
            ]}])
            write_json(job / "result.json", {"finished_at": "2026-01-01", "n_total_trials": 1})
            write_json(trial / "result.json", {
                "task_name": "test-task", "verifier_result": {"rewards": {"reward": 0}},
            })
            write_json(artifacts / "manifest.json", [
                {"source": "/app/answer.txt", "destination": "artifacts/answer.txt", "status": "ok"},
            ])
            atif = agent / "trajectory.json"
            answer = artifacts / "answer.txt"
            atif.write_text("{}")
            answer.write_text("Wrong answer with complete evidence.")

            def summarize_run():
                summarize(run, "fixture", 1, 0, False, True, 0,
                          "test-task", job.name, registry, "harbor", job.name)

            summarize_run()
            summary = run / "summary.json"
            self.assertTrue(read_json(summary)["complete"])
            for artifact, message in [(atif, "missing ATIF"), (answer, "missing a required submission")]:
                with self.subTest(artifact=artifact.name):
                    content = artifact.read_text()
                    artifact.unlink()
                    with self.assertRaisesRegex(SystemExit, message):
                        summarize_run()
                    result = read_json(summary)
                    self.assertFalse(result["complete"])
                    self.assertEqual(result["meanScore"], 0)
                    artifact.write_text(content)


if __name__ == "__main__":
    unittest.main()
