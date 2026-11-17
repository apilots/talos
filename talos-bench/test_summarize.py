import unittest
from pathlib import Path
from tempfile import TemporaryDirectory

from summarize import (
    estimate_cost, read_json, root_cache_read, root_estimated_cost, summarize, trial_summary, workload_usage, write_json,
)


class CompletionTest(unittest.TestCase):
    def test_estimate_cost_uses_reported_zero_values_and_requires_known_counters(self):
        rates = {"input": 2, "output": 3, "cache_read": 1, "cache_write": 4}
        usage = {"input_tokens": 50, "output_tokens": 4, "cache_read_input_tokens": 0,
                 "cache_creation_input_tokens": 5}
        self.assertAlmostEqual(estimate_cost(usage, rates), 122 / 1_000_000)
        self.assertIsNone(estimate_cost({**usage, "cache_read_input_tokens": None}, rates))

    def test_root_cost_rejects_partial_cache_totals_even_when_observed_sums_match(self):
        rates = {"input": 2, "output": 3, "cache_read": 1, "cache_write": 4}
        first = {"input_tokens": 20, "output_tokens": 2, "cache_read_input_tokens": 3,
                 "cache_creation_input_tokens": 0}
        second = {**first, "cache_creation_input_tokens": 1}
        requests = [{"callRecords": [{"metrics": {"usage": counts}}]} for counts in (first, second)]
        totals = {"modelRounds": 2, "usage": {key: first[key] + second[key] for key in first}}
        self.assertAlmostEqual(root_estimated_cost(requests, totals, rates), 88 / 1_000_000)
        self.assertIsNone(root_estimated_cost(requests, {**totals, "modelRounds": 3}, rates))
        first["cache_creation_input_tokens"] = None
        self.assertIsNone(root_estimated_cost(requests, totals, rates))

    def test_workload_only_reports_aggregate_when_all_child_turns_reconcile(self):
        root_cache = {"status": "verified", "coveredInputTokens": 100, "observedCacheReadTokens": 50}
        root = {"model": "test:selected", "exit_code": 0,
                "prices_per_million": {"input": 2, "output": 3, "cache_read": 1, "cache_write": 4}}
        child = {"path": "/root/check", "turnId": "child"}
        child_usage = {"input_tokens": 50, "output_tokens": 4, "cache_read_input_tokens": 20,
                       "cache_creation_input_tokens": 5}
        snapshot = {
            "fields": ["path", "turnId", "event"],
            "events": [[None, "root", "turnStarted"], [child["path"], "child", "turnStarted"]],
            "activeTurns": [],
            "observedTurnMetrics": [{**child, "modelRounds": 1, "usage": child_usage}],
            "childUsage": [{**child, "reportedInputTokens": 50, "reportedCacheReadTokens": 20,
                            "observedCalls": 1, "callsWithReportedCacheRead": 1,
                            "coveredInputTokens": 50, "observedCacheReadTokens": 20,
                            "observedRequests": 1, "callsWithCompleteUsage": 1,
                            "observedUsage": child_usage, "reportedUsage": child_usage}],
        }
        whole = workload_usage(snapshot, "root", root_cache, 0.001, root)
        self.assertEqual(whole["observedDescendantUsage"], child_usage)
        self.assertEqual(whole["descendantTurnsReconciled"], 1)
        self.assertAlmostEqual(whole["cacheReadRatio"], 70 / 150)
        self.assertAlmostEqual(whole["estimatedCostUsd"], 0.001 + 102 / 1_000_000)

        # An omitted optional cache write can leave the reported totals numerically unchanged.
        evidence, = snapshot["childUsage"]
        snapshot["childUsage"] = [{**evidence, "callsWithCompleteUsage": 0}]
        incomplete = workload_usage(snapshot, "root", root_cache, 0.001, root)
        self.assertAlmostEqual(incomplete["cacheReadRatio"], 70 / 150)
        self.assertEqual(incomplete["pricedDescendantTurns"], 0)
        self.assertIsNone(incomplete["estimatedCostUsd"])

        # Historical input/read evidence can support only an estimate from reported totals.
        snapshot["childUsage"] = [{name: value for name, value in evidence.items()
                                   if name not in {"observedRequests", "callsWithCompleteUsage",
                                                   "observedUsage", "reportedUsage"}}]
        historical = workload_usage(snapshot, "root", root_cache, 0.001, root)
        self.assertAlmostEqual(historical["cacheReadRatio"], 70 / 150)
        self.assertIsNone(historical["estimatedCostUsd"])
        self.assertEqual(historical["descendantTurnsEstimatedFromReportedTotals"], 1)
        self.assertAlmostEqual(historical["descendantEstimateFromReportedTotalsUsd"], 102 / 1_000_000)

        # A child may settle before its usage becomes available, such as on cancellation.
        snapshot["childUsage"] = [evidence]
        snapshot["events"].append(["/root/cancelled", "cancelled", "turnStarted"])
        partial = workload_usage(snapshot, "root", root_cache, 0.001, root)
        self.assertEqual(partial["descendantTurnsObserved"], 2)
        self.assertEqual(partial["descendantTurnsWithUsage"], 1)
        self.assertEqual(partial["descendantTurnsReconciled"], 1)
        self.assertAlmostEqual(partial["reconciledDescendantEstimatedCostUsd"], 102 / 1_000_000)
        self.assertIsNone(partial["cacheReadRatio"])
        self.assertIsNone(partial["estimatedCostUsd"])

    def test_root_cache_share_requires_matching_input_coverage_and_counters(self):
        calls = [
            {"metrics": {"usage": {"input_tokens": 90, "cache_read_input_tokens": 0}}},
            {"metrics": {"usage": {"input_tokens": 10, "cache_read_input_tokens": 10}}},
        ]
        totals = {"modelRounds": 2, "usage": {"input_tokens": 100, "cache_read_input_tokens": 10}}
        self.assertEqual(root_cache_read(calls, totals, 2), {
            "status": "verified", "ratio": 0.1, "observedCalls": 2, "callsWithReportedCacheRead": 2,
            "coveredInputTokens": 100, "observedCacheReadTokens": 10,
        })
        all_misses = {"modelRounds": 1, "usage": calls[0]["metrics"]["usage"]}
        self.assertEqual(root_cache_read(calls[:1], all_misses, 1)["ratio"], 0)
        for label, sample, metrics, observed, status in (
            ("missing cache counter", [{"metrics": {"usage": {"input_tokens": 90}}}, calls[1]],
             totals, 2, "partial"),
            ("truncated trajectory", calls, {**totals, "modelRounds": 3}, 2, "partial"),
            ("missing CLI usage", calls, {}, 2, "partial"),
            ("unreported reads still contribute input", [*calls, {"metrics": {"usage": {"input_tokens": 1}}}],
             totals, 3, "inconsistent"),
            ("missing trajectory", [], totals, 0, "unavailable"),
            ("inconsistent totals", calls, {**totals, "usage": {"input_tokens": 100,
                                                             "cache_read_input_tokens": 11}}, 2, "inconsistent"),
        ):
            with self.subTest(label=label):
                evidence = root_cache_read(sample, metrics, observed)
                self.assertEqual(evidence["status"], status)
                self.assertIsNone(evidence["ratio"])

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
                    "callRecords": [{"metrics": {"executionDurationMs": 1500, "usage": {
                        "input_tokens": 20, "output_tokens": 3, "cache_read_input_tokens": 6,
                        "cache_creation_input_tokens": 7,
                    }}}],
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
