import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from run_talos import capture_answer, capture_provider_retries


class CaptureAnswerTests(unittest.TestCase):
    def test_only_the_matching_terminal_footer_is_removed(self):
        identity = {"sessionId": "session-1", "turnId": "turn-1"}
        footer = json.dumps({**identity, "status": "completed"}).encode() + b"\n"
        other = json.dumps({**identity, "turnId": "answer", "status": "completed"}).encode() + b"\n"
        with TemporaryDirectory() as directory:
            logs = Path(directory)
            (logs / "talos-trajectory.json").write_text(json.dumps({"identity": identity}))
            raw = logs / "talos.stdout.txt"
            answer = logs / "answer.txt"
            for stdout, expected in (
                (b"first line\nsecond line\n" + footer, b"first line\nsecond line\n"),
                (other + footer, other),
                (footer + footer, footer),
                (other, other),
                (b"legacy answer\n", b"legacy answer\n"),
                (footer, b""),
            ):
                with self.subTest(stdout=stdout):
                    raw.write_bytes(stdout)
                    capture_answer(answer, logs)
                    self.assertEqual(answer.read_bytes(), expected)
                    self.assertEqual(raw.read_bytes(), stdout)


class ProviderRetryEvidenceTests(unittest.TestCase):
    def test_projects_only_typed_retry_fields_from_rotated_json_logs(self):
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            private = root / "home/logs"
            private.mkdir(parents=True)
            logs = root / "agent"
            logs.mkdir()
            event = {"timestamp": "2026-11-18T00:00:00Z", "target": "talos_llm_openai",
                     "fields": {"attempt": 1, "http_status": 502, "message": "private diagnostic"}}
            (private / "talos.log.1").write_text(json.dumps(event) + "\n")
            event["fields"]["attempt"] = 2
            event["fields"]["http_status"] = 503
            (private / "talos.log").write_text(json.dumps(event) + "\n{partial")
            events = capture_provider_retries(root / "home", logs)
            self.assertEqual(sorted(x["attempt"] for x in events), [1, 2])
            self.assertEqual({x["http_status"] for x in events}, {502, 503})
            self.assertTrue(all(set(x) == {"timestamp", "attempt", "http_status"} for x in events))
            self.assertNotIn("private diagnostic", (logs / "provider-retries.json").read_text())

    def test_unrelated_or_invalid_records_do_not_become_retry_evidence(self):
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "home/logs").mkdir(parents=True)
            logs = root / "agent"
            logs.mkdir()
            records = [
                {"target": "other", "fields": {"attempt": 1, "http_status": 502}},
                {"target": "talos_llm_openai", "fields": {"attempt": "1", "http_status": 502}},
                {"target": "talos_llm_openai", "fields": {"attempt": 1, "http_status": 401}},
                {"target": "talos_llm_openai", "fields": {"attempt": 0, "http_status": 502}},
            ]
            for record in records:
                record["timestamp"] = "2026-11-18T00:00:00Z"
            (root / "home/logs/talos.log").write_text("\n".join(map(json.dumps, records)))
            self.assertEqual(capture_provider_retries(root / "home", logs), [])
            self.assertEqual(json.loads((logs / "provider-retries.json").read_text()), [])
