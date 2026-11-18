import importlib.util
import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest
import zipfile

spec = importlib.util.spec_from_file_location("runta_collect", Path(__file__).with_name("runta_collect.py"))
collector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(collector)


class CollectionTest(unittest.TestCase):
    def test_preserves_original_reward_when_summary_excludes_timeout(self):
        with TemporaryDirectory() as temp:
            root = Path(temp)
            batch = root / "batch"
            case = batch / "cases/terminal-bench--a"
            case.mkdir(parents=True)
            (batch / "manifest.json").write_text(json.dumps({"tasks": ["terminal-bench/a"]}))
            result = root / "result"
            result.mkdir()
            archive = result / "original-job.tar.gz"
            archive.write_bytes(b"raw job")
            (result / "summary.json").write_text(json.dumps({"trials": [{"score": None,
                "verifierReward": 1.0, "exception": "AgentTimeoutError"}]}))
            (case / "state.json").write_text(json.dumps({"status": "finished", "output": str(result),
                "archive_sha256": collector.sha256(archive)}))
            rows = collector.collect(batch, root / "bundle.zip")
            self.assertEqual(rows[0]["trial_evidence"][0]["verifierReward"], 1.0)
            self.assertIsNone(rows[0]["trial_evidence"][0]["score"])
            with zipfile.ZipFile(root / "bundle.zip") as z:
                self.assertEqual(z.read(rows[0]["archive"]), b"raw job")

    def test_missing_or_changed_archives_are_not_claimed(self):
        with TemporaryDirectory() as temp:
            root = Path(temp)
            batch = root / "batch"
            case = batch / "cases/terminal-bench--a"
            case.mkdir(parents=True)
            (batch / "manifest.json").write_text(json.dumps({"tasks": ["terminal-bench/a", "terminal-bench/b"],
                                                               "secret_name": "private-name"}))
            result = root / "result"
            result.mkdir()
            (result / "original-job.tar.gz").write_bytes(b"wrong")
            (case / "state.json").write_text(json.dumps({"status": "finished", "output": str(result),
                                                        "archive_sha256": "wrong"}))
            rows = collector.collect(batch, root / "bundle.zip")
            self.assertIsNone(rows[0]["archive"])
            self.assertEqual(rows[1]["status"], "not_started")
            with zipfile.ZipFile(root / "bundle.zip") as z:
                self.assertNotIn("private-name", z.read("manifest.json").decode())
                self.assertEqual(z.namelist(), ["manifest.json"])


if __name__ == "__main__":
    unittest.main()
