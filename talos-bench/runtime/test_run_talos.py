import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from run_talos import capture_answer


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
