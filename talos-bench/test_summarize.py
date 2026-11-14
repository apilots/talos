import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from summarize import read_json, summarize, trial_summary, write_json


class CompletionTest(unittest.TestCase):
    def test_interrupted_cli_preserves_launch_profile_and_runner_duration(self):
        with TemporaryDirectory() as directory:
            run = Path(directory)
            trial = run / "task__trial"
            agent = trial / "agent"
            agent.mkdir(parents=True)
            write_json(agent / "talos-run.json", {
                "model": "test:model",
                "request_interval_ms": "6000",
                "elapsed_seconds": None,
                "exit_code": None,
            })
            result = {
                "task_name": "task",
                "agent_execution": {"started_at": "2026-01-01T00:00:00Z", "finished_at": "2026-01-01T00:01:30Z"},
                "verifier_result": {"rewards": {"reward": 0}},
                "exception_info": {"exception_type": "AgentTimeoutError"},
            }
            write_json(trial / "result.json", result)
            actual = trial_summary(run, trial, {})
            self.assertEqual(actual["exception"], "AgentTimeoutError")
            self.assertEqual(actual["agentSeconds"], 90)
            self.assertEqual(actual["requestIntervalMs"], "6000")
            self.assertEqual(actual["verifierReward"], 0)
            self.assertIsNone(actual["score"])
            self.assertIsNone(actual["cliSeconds"])

            del result["exception_info"]
            write_json(trial / "result.json", result)
            actual = trial_summary(run, trial, {})
            self.assertEqual(actual["exception"], "TalosCliTerminationUnrecorded")
            self.assertIsNone(actual["score"])

    def test_same_path_from_two_compose_services_requires_both_artifacts(self):
        with TemporaryDirectory() as directory:
            run = Path(directory)
            job = run / "test-suite"
            trial = job / "test-task__trial"
            artifacts = trial / "artifacts"
            artifacts.mkdir(parents=True)
            registry = run / "registry.json"
            source = "/app/answer.txt"
            write_json(registry, [{"version": "1.0", "tasks": [{
                "name": "test-task",
                "submission_paths": [source, {"source": source, "service": "sidecar"}],
            }]}])
            write_json(job / "result.json", {"finished_at": "2026-01-01", "n_total_trials": 1})
            write_json(trial / "result.json", {
                "task_name": "test-task", "verifier_result": {"rewards": {"reward": 0}},
            })
            (artifacts / "answer.txt").write_text("main")
            (artifacts / "sidecar.txt").write_text("sidecar")
            captured = [
                {"source": source, "destination": "artifacts/answer.txt", "status": "ok"},
                {"source": source, "service": "sidecar", "destination": "artifacts/sidecar.txt", "status": "ok"},
            ]

            def summarize_run():
                summarize(run, "real", 1, 0, False, False, None, "test-task",
                          job.name, registry, "harbor", job.name)

            write_json(artifacts / "manifest.json", captured)
            summarize_run()
            summary = read_json(run / "summary.json")
            main, sidecar = summary["trials"][0]["submissions"]
            self.assertTrue(summary["complete"])
            self.assertEqual((run / main["path"]).read_text(), "main")
            self.assertEqual((run / sidecar["path"]).read_text(), "sidecar")
            write_json(artifacts / "manifest.json", captured[:1])
            with self.assertRaisesRegex(SystemExit, "missing a required submission"):
                summarize_run()

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
            write_json(agent / "talos-run.json", {
                "collaboration_tools": False,
                "prices_per_million": {"input": 2, "output": 3, "cache_read": 1, "cache_write": 4},
            })
            (agent / "talos.stderr.txt").write_text(
                'TOOL_CALL_COMPLETED call_id=call tool=Bash internal=Bash is_error=false '
                'metrics={"executionDurationMs":250,"queueDurationMs":50}\n'
                'TALOS_EXEC_METRICS {"modelRounds":1,"usage":{"total_tokens":23,"input_tokens":20,'
                '"output_tokens":3,"cache_read_input_tokens":6,"cache_creation_input_tokens":7}}\n'
            )
            answer.write_text("Wrong answer with complete evidence.")

            def summarize_run():
                summarize(run, "fixture", 1, 0, True, True, 0,
                          "test-task", job.name, registry, "harbor", job.name)

            summarize_run()
            summary = run / "summary.json"
            result = read_json(summary)
            self.assertTrue(result["complete"])
            self.assertEqual(result["passRate"], 0)
            self.assertIsNone(result["estimatedCostPerPassUsd"])
            self.assertAlmostEqual(result["meanEstimatedCostUsd"], 0.000057)
            trial_summary, = result["trials"]
            self.assertIs(trial_summary["collaborationTools"], False)
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
                    self.assertIsNone(result["meanScore"])
                    self.assertEqual(result["scoredTrials"], 0)
                    self.assertIsNone(result["trials"][0]["score"])
                    self.assertEqual(result["trials"][0]["verifierReward"], 0)
                    artifact.write_text(content)

            (agent / "talos-delegation.jsonl").write_text(
                '{"stopReason":"recursiveDelegationWithoutProgress"}\n{"captureError":"SQLITE_BUSY"}\n'
            )
            write_json(agent / "talos-delegation-events.json", {"events": []})
            trace.write_text("")
            (agent / "talos-transcript.json").touch()
            with self.assertRaisesRegex(SystemExit, "did not score every trial"):
                summarize_run()
            result = read_json(summary)
            trial_summary, = result["trials"]
            self.assertIsNone(trial_summary["score"])
            self.assertEqual(trial_summary["verifierReward"], 0)
            self.assertIn("delegationEvents", trial_summary["artifacts"])
            self.assertNotIn("trajectory", trial_summary["artifacts"])
            self.assertNotIn("transcript", trial_summary["artifacts"])


if __name__ == "__main__":
    unittest.main()
