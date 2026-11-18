import json
from pathlib import Path
from tempfile import TemporaryDirectory
import unittest

from atif import export_atif
from harbor.utils.trajectory_validator import TrajectoryValidator


class FailedAtifTest(unittest.TestCase):
    def test_failed_request_preserves_user_only_history_and_failure_metadata(self):
        with TemporaryDirectory() as temporary:
            logs = Path(temporary)
            (logs / "talos-run.json").write_text(json.dumps({
                "exit_code": 1, "model": "openai:test", "version": "test",
            }))
            (logs / "talos-trajectory.json").write_text(json.dumps({
                "identity": {"sessionId": "session", "turnId": "turn"}, "data": [],
            }))
            (logs / "talos-transcript.json").write_text(json.dumps([{
                "startedAt": 1, "content": {"type": "userContent", "data": {"text": "task"}},
            }]))
            export_atif(logs)
            document = json.loads((logs / "trajectory.json").read_text())
            self.assertTrue(TrajectoryValidator().validate(document))
            self.assertEqual([s["source"] for s in document["steps"]], ["user"])
            self.assertEqual(document["extra"]["talos_exit_code"], 1)
            self.assertEqual(document["extra"]["talos_execution_status"], "failed")
            self.assertEqual(json.loads((logs / "talos-run.json").read_text())["exit_code"], 1)

    def test_interruption_preserves_completed_tool_calls(self):
        with TemporaryDirectory() as temporary:
            logs = Path(temporary)
            (logs / "talos-run.json").write_text(json.dumps({
                "exit_code": None, "model": "openai:test", "version": "test",
            }))
            (logs / "talos-trajectory.json").write_text(json.dumps({
                "identity": {"sessionId": "session", "turnId": "turn"}, "data": [],
            }))
            blocks = [
                ("userContent", {"text": "task"}),
                ("assistantText", {"text": "Inspecting the file."}),
                ("toolCall", {"callId": "call", "name": "shell", "arguments": {"value": {"command": "pwd"}}}),
                ("toolResult", {"callId": "call", "content": "output", "isError": False}),
            ]
            (logs / "talos-transcript.json").write_text(json.dumps([
                {"startedAt": i + 1, "content": {"type": kind, "data": data}}
                for i, (kind, data) in enumerate(blocks)
            ]))
            export_atif(logs)
            document = json.loads((logs / "trajectory.json").read_text())
            self.assertTrue(TrajectoryValidator().validate(document))
            self.assertEqual(len(document["steps"]), 2)
            agent = document["steps"][1]
            self.assertEqual(agent["message"], "Inspecting the file.")
            self.assertEqual(agent["tool_calls"][0]["function_name"], "shell")
            self.assertEqual(agent["observation"]["results"][0]["source_call_id"], "call")
            self.assertEqual(document["extra"]["talos_execution_status"], "interrupted")
            self.assertIsNone(document["extra"]["talos_exit_code"])

    def test_success_without_agent_output_remains_invalid(self):
        with TemporaryDirectory() as temporary:
            logs = Path(temporary)
            (logs / "talos-run.json").write_text(json.dumps({"exit_code": 0}))
            (logs / "talos-trajectory.json").write_text(json.dumps({"identity": {"sessionId": "s"}}))
            (logs / "talos-transcript.json").write_text(json.dumps([{
                "startedAt": 1, "content": {"type": "userContent", "data": {"text": "task"}},
            }]))
            with self.assertRaises(ValueError):
                export_atif(logs)


if __name__ == "__main__":
    unittest.main()
