import io
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from harbor.models.model_api_format import ModelApiFormat
from harbor.utils.trajectory_validator import TrajectoryValidator

from arena_agent import CustomAgent, download, installation_command, talos_connection


def connection(api_format=ModelApiFormat.OPENAI_RESPONSES):
    return SimpleNamespace(model_api_format=api_format, model_id="example/model",
                           execution_base_url="http://localhost:4011/v1", api_key="stub-$key")


class ConnectionTests(unittest.TestCase):
    def test_model_endpoint_protocol_and_effort_reach_talos_configuration(self):
        llm, credential = talos_connection(connection(), "high")
        self.assertEqual(llm["model"], "example/model")
        self.assertEqual(llm["custom_providers"]["arena"]["base_url"], "http://localhost:4011/v1")
        self.assertEqual(llm["custom_providers"]["arena"]["protocol"], "responses")
        self.assertEqual(llm["custom_providers"]["arena"]["models"][0]["reasoning_effort"], "high")
        self.assertEqual(credential, 'TALOS_LLM_OPENAI_API_KEY="stub-\\$key"')

    def test_low_effort_maps_to_talos_light(self):
        llm, _ = talos_connection(connection(), "low")
        self.assertEqual(llm["custom_providers"]["arena"]["models"][0]["reasoning_effort"], "light")

    def test_unrepresentable_efforts_are_rejected(self):
        for effort in ("none", "minimal", "max"):
            with self.subTest(effort=effort), self.assertRaises(ValueError):
                talos_connection(connection(), effort)

    def test_chat_omits_unspecified_effort(self):
        llm, _ = talos_connection(connection(ModelApiFormat.OPENAI_CHAT_COMPLETIONS), None)
        self.assertEqual(llm["custom_providers"]["arena"]["protocol"], "chat")
        self.assertNotIn("reasoning_effort", llm["custom_providers"]["arena"]["models"][0])

    def test_unsupported_protocol_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "supports OpenAI"):
            talos_connection(connection(ModelApiFormat.ANTHROPIC_MESSAGES), None)

    def test_harbor_overrides_resolve_for_bare_model(self):
        with TemporaryDirectory() as directory:
            agent = CustomAgent(logs_dir=Path(directory), model_name="test-model",
                                base_url="https://gateway.example/v1",
                                model_api_key_name="ARENA_KEY",
                                model_api_format="openai_responses",
                                extra_env={"ARENA_KEY": "supplied-stub"}, reasoning_effort="high")
            llm, credential = talos_connection(agent.model_connection, agent.effort)
            self.assertEqual(llm["custom_providers"]["arena"]["base_url"], "https://gateway.example/v1")
            self.assertIn("supplied-stub", credential)


class ArtifactTests(unittest.TestCase):
    def test_corrupt_download_is_rejected(self):
        with TemporaryDirectory() as directory, patch("arena_agent.urlopen", return_value=io.BytesIO(b"bad")):
            with self.assertRaisesRegex(ValueError, "checksum mismatch"):
                download("https://example.invalid/product", Path(directory) / "product", "0" * 64, 100)

    def test_download_limit_is_enforced(self):
        with TemporaryDirectory() as directory, patch("arena_agent.urlopen", return_value=io.BytesIO(b"oversize")):
            with self.assertRaisesRegex(ValueError, "download limit"):
                download("https://example.invalid/product", Path(directory) / "product", "0" * 64, 1)


class InstallationTests(unittest.TestCase):
    def test_pinned_archive_installs_and_corruption_is_rejected(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            archive = root / "Talos-TUI-0.1.18-linux-amd64.tar.gz"
            data = b"#!/bin/sh\necho 'talos 0.1.18'\n"
            with tarfile.open(archive, "w:gz") as package:
                member = tarfile.TarInfo("bin/talos")
                member.size, member.mode = len(data), 0o755
                package.addfile(member, io.BytesIO(data))
            checksum = hashlib.sha256(archive.read_bytes()).hexdigest()
            with patch("arena_agent.RELEASE_ROOT", root.as_uri()), patch(
                    "arena_agent.PRODUCTS", {"x86_64": ("amd64", checksum)}):
                result = subprocess.run(installation_command("x86_64", str(root / "installed")),
                                        shell=True, capture_output=True, text=True)
                self.assertEqual((result.returncode, result.stdout.strip()), (0, "talos 0.1.18"))
                archive.write_bytes(b"corrupt archive")
                result = subprocess.run(installation_command("x86_64", str(root / "rejected")),
                                        shell=True, capture_output=True, text=True)
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("checksum mismatch", result.stderr)
                self.assertFalse((root / "rejected/bin/talos").exists())


class ExecutionTests(unittest.IsolatedAsyncioTestCase):
    async def test_failed_execution_preserves_shared_atif_and_original_exit(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            remote = root / "remote"
            remote.mkdir()
            (remote / "talos-run.json").write_text(json.dumps({
                "exit_code": 7, "model": "openai:test", "version": "0.1.18"}))
            (remote / "talos-trajectory.json").write_text(json.dumps({
                "identity": {"sessionId": "session", "turnId": "turn"}}))
            (remote / "talos-transcript.json").write_text(json.dumps([
                {"startedAt": 1, "content": {"type": "userContent", "data": {"text": "task"}}},
                {"startedAt": 2, "content": {"type": "toolCall", "data": {
                    "callId": "call", "name": "shell", "arguments": {"value": {"command": "pwd"}}}}},
                {"startedAt": 3, "content": {"type": "toolResult", "data": {
                    "callId": "call", "content": "workspace", "isError": False}}},
            ]))
            class Environment:
                async def exec(self, command):
                    return SimpleNamespace(return_code=7)

                async def download_dir(self, source, destination):
                    shutil.copytree(remote, destination, dirs_exist_ok=True)

            agent = CustomAgent(logs_dir=root / "logs", model_name="openai/test")
            agent.product = Path(__file__).resolve().parent
            from arena_agent import load_payload
            agent.payload = load_payload(agent.product)
            with self.assertRaisesRegex(RuntimeError, "code 7"):
                await agent.run("task", Environment(), SimpleNamespace())
            document = json.loads((agent.logs_dir / "trajectory.json").read_text())
            self.assertTrue(TrajectoryValidator().validate(document))
            self.assertEqual(document["extra"]["talos_exit_code"], 7)
            self.assertEqual(document["steps"][1]["observation"]["results"][0]["source_call_id"], "call")


if __name__ == "__main__":
    unittest.main()
