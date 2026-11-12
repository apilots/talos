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
            trace = agent / "talos-trajectory.json"
            answer = artifacts / "answer.txt"
            atif.write_text("{}")
            write_json(trace, {
                "identity": {"sessionId": "session", "turnId": "turn"},
                "clockSegments": [],
                "data": [{"type": "modelRequest", "request": {
                    "callRecords": [{"metrics": {"executionDurationMs": 1500}}],
                }}],
                "nextCursor": None,
            })
            (agent / "talos.stderr.txt").write_text(
                'TOOL_CALL_COMPLETED call_id=call tool=Bash internal=Bash is_error=false '
                'metrics={"executionDurationMs":250,"queueDurationMs":50}\n'
                'TALOS_EXEC_METRICS {"modelRounds":1,"usage":{"total_tokens":23}}\n'
            )
            answer.write_text("Wrong answer with complete evidence.")

            def summarize_run():
                summarize(run, "fixture", 1, 0, True, True, 0,
                          "test-task", job.name, registry, "harbor", job.name)

            summarize_run()
            summary = run / "summary.json"
            result = read_json(summary)
            self.assertTrue(result["complete"])
            trial_summary, = result["trials"]
            self.assertEqual(trial_summary["usage"]["total_tokens"], 23)
            self.assertEqual(trial_summary["observedModelSeconds"], 1.5)
            self.assertEqual(trial_summary["observedToolSeconds"], 0.25)
            self.assertEqual(trial_summary["observedToolQueueSeconds"], 0.05)
            for artifact, message in [(trace, "missing trajectory metadata"),
                                      (atif, "missing ATIF"),
                                      (answer, "missing a required submission")]:
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
