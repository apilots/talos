from pathlib import Path
import json
import sqlite3
import subprocess
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from delegation_watch import wait, worktree_signature


class DelegationWatchTest(unittest.TestCase):
    def test_task_without_git_or_declared_paths_has_no_progress_signal(self):
        with TemporaryDirectory() as directory:
            self.assertIsNone(worktree_signature(Path(directory), []))

    def test_stops_running_process_and_records_recursive_growth(self):
        with TemporaryDirectory() as directory:
            home = Path(directory)
            database = home / "data/talos.db"
            database.parent.mkdir()
            with sqlite3.connect(database) as db:
                db.execute("CREATE TABLE sessions (session_key TEXT, agent_path TEXT)")
                db.execute("""
                    CREATE TABLE agent_events (session_id TEXT, turn_id TEXT, sequence INTEGER,
                                               occurred_at INTEGER, payload_json TEXT, event_seq INTEGER)
                """)
                db.executemany("INSERT INTO sessions VALUES (?, ?)", [(str(i), f"/root/a/b/c/{i}") for i in range(12)])
                tool_event = {"type": "messageBlockCompleted", "data": {"block": {"content": {
                    "type": "toolCall", "data": {"name": "spawn_agent", "callId": "spawn-1",
                                                "arguments": "Excluded from diagnostics"},
                }}}}
                db.execute("INSERT INTO agent_events VALUES ('0', 'child', 1, 1, ?, 1)", (json.dumps(tool_event),))
                result_event = {"type": "messageBlockCompleted", "data": {"block": {"content": {
                    "type": "toolResult", "data": {"callId": "spawn-1", "isError": False,
                                                  "metrics": {"executionDurationMs": 12, "queueDurationMs": 3}},
                }}}}
                db.execute("INSERT INTO agent_events VALUES ('0', 'child', 2, 1, ?, 2)", (json.dumps(result_event),))
            paths = ["submission"]
            submission = home / paths[0] / "output.txt"
            submission.parent.mkdir()
            submission.write_text("Initial output")
            signature = worktree_signature(home, paths)
            with subprocess.Popen([sys.executable, "-c", "import time; time.sleep(60)"]) as process:
                wait_for_process = process.wait

                def advance(timeout):
                    if timeout == 10:
                        return wait_for_process(timeout)
                    with sqlite3.connect(database) as db:
                        index, = db.execute("SELECT COUNT(*) FROM sessions").fetchone()
                        db.execute("INSERT INTO sessions VALUES (?, ?)", (str(index), f"/root/a/b/c/{index}"))
                    if index == 13:
                        submission.write_text("Deliverable progress")
                        self.assertTrue((home / "talos-delegation-events.json").is_file())
                    raise subprocess.TimeoutExpired(process.args, timeout)

                with patch.object(process, "wait", side_effect=advance), patch(
                    "delegation_watch.time.monotonic", side_effect=[0, 60, 540, 600, 660, 720, 1080, 1140, 1200, 1260]
                ):
                    self.assertEqual(wait(process, home, home, home, paths, signature), 124)
                self.assertIsNotNone(process.returncode)
            samples = (home / "talos-delegation.jsonl").read_text().splitlines()
            self.assertEqual(json.loads(samples[1])["secondsWithoutProgress"], 0)
            self.assertEqual(json.loads(samples[-1])["stopReason"], "recursiveDelegationWithoutProgress")
            capture = (home / "talos-delegation-events.json").read_text()
            self.assertEqual(json.loads(capture)["events"], [
                ["/root/a/b/c/0", "child", 1, 1, "messageBlockCompleted", "toolCall", "spawn_agent", "spawn-1", None, None, 1, None, None],
                ["/root/a/b/c/0", "child", 2, 1, "messageBlockCompleted", "toolResult", None, "spawn-1", 0, None, 2, 12, 3],
            ])
            self.assertNotIn("Excluded from diagnostics", capture)
