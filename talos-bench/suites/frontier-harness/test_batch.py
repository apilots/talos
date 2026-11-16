import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from batch import outcome_from_summary


class OutcomeTest(unittest.TestCase):
    def test_reward_excluded_when_browser_verifier_cannot_start(self):
        with TemporaryDirectory() as directory:
            case = Path(directory)
            verifier = case / "verifier"
            verifier.mkdir()
            (verifier / "test-stdout.txt").write_text(
                "Failed to create driver or process file: Message: Unable to obtain driver for chrome\n"
            )
            summary = case / "summary.json"
            for reward in (0.0, 1.0):
                with self.subTest(reward=reward):
                    summary.write_text(json.dumps({"complete": True, "trials": [{
                        "score": reward, "verifierReward": reward, "artifacts": {"verifier": "verifier"},
                    }]}))
                    result = outcome_from_summary(case, "terminal-bench/filter-js-from-html", summary, 1)
                    self.assertEqual(result["status"], "infraFailure")
                    self.assertEqual(result["reason"], "verifierBrowserDriverUnavailable")
                    self.assertEqual(result["officialReward"], reward)
                    self.assertIsNone(result["reward"])
