import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from run_talos import capture_answer, capture_provider_http_failures


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
    def test_retains_retries_and_terminal_failures_without_remote_text(self):
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            private = root / "home/logs"
            private.mkdir(parents=True)
            logs = root / "agent"
            logs.mkdir()
            fields = {"event": "http_rejection", "protocol": "Responses", "attempt": 5,
                      "max_attempts": 6, "elapsed_ms": 80, "http_status": 502,
                      "will_retry": True, "upstream_request_id": "req-502", "message": "private diagnostic"}
            retry = {"timestamp": "2026-11-18T00:00:00Z", "target": "talos_llm_openai", "fields": fields}
            terminal = {**retry, "timestamp": "2026-11-18T00:00:00.100000001Z",
                        "fields": {**fields, "attempt": 6, "http_status": 504, "will_retry": False,
                                   "upstream_request_id": "https://example.invalid/private"}}
            timeout = {**retry, "timestamp": "2026-11-18T00:00:06.1Z",
                       "fields": {"event": "transport_failure", "protocol": "Chat", "attempt": 1,
                                  "max_attempts": 6, "elapsed_ms": 120000, "failure": "timeout"}}
            (private / "talos.log.1").write_text(json.dumps(retry) + "\n")
            (private / "talos.log").write_text(json.dumps(terminal) + "\n" + json.dumps(timeout) + "\n{partial")
            self.assertEqual(capture_provider_http_failures(root / "home", logs), 1)
            preserved = (logs / "provider-http-failures.json").read_text()
            failures = json.loads(preserved)
            self.assertEqual(failures, [
                {"timestamp": retry["timestamp"], "protocol": "Responses", "attempt": 5,
                 "max_attempts": 6, "elapsed_ms": 80, "http_status": 502,
                 "will_retry": True, "upstream_request_id": "req-502"},
                {"timestamp": terminal["timestamp"], "protocol": "Responses", "attempt": 6,
                 "max_attempts": 6, "elapsed_ms": 80, "http_status": 504, "will_retry": False},
                {"timestamp": timeout["timestamp"], "protocol": "Chat", "attempt": 1,
                 "max_attempts": 6, "elapsed_ms": 120000, "failure": "timeout", "will_retry": False},
            ])
            self.assertNotIn("private", preserved)

    def test_unrelated_or_invalid_records_do_not_become_retry_evidence(self):
        with TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "home/logs").mkdir(parents=True)
            logs = root / "agent"
            logs.mkdir()
            fields = {"event": "http_rejection", "protocol": "Responses", "attempt": 1,
                      "max_attempts": 6, "elapsed_ms": 42, "http_status": 502, "will_retry": True}
            original = {"target": "talos_llm_openai", "timestamp": "2026-11-18T00:00:00Z", "fields": fields}
            records = [
                {**original, "target": "other"},
                {**original, "fields": {**fields, "attempt": "1"}},
                {**original, "fields": {**fields, "http_status": 401}},
                {**original, "fields": {**fields, "attempt": 6}},
                {**original, "fields": {**fields, "event": "other"}},
            ]
            (root / "home/logs/talos.log").write_text("\n".join(map(json.dumps, records)))
            self.assertEqual(capture_provider_http_failures(root / "home", logs), 0)
            self.assertEqual(json.loads((logs / "provider-http-failures.json").read_text()), [])
