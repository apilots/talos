"""Exercise the Pier configuration used for Talos agent environment variables."""

from pathlib import Path
from tempfile import TemporaryDirectory
from types import SimpleNamespace
import unittest
from unittest.mock import AsyncMock, patch

from pier.agents.factory import AgentFactory
from pier.models.trial.config import AgentConfig


class AgentEnvironmentTest(unittest.IsolatedAsyncioTestCase):
    async def test_pier_passes_agent_env_to_talos(self):
        with TemporaryDirectory() as directory:
            logs = Path(directory)
            (logs / "talos.stderr.txt").touch()
            config = AgentConfig(
                import_path="talos_agent:TalosAgent",
                kwargs={"product": directory},
                env={"TALOS_BENCH_OPENAI_REQUEST_INTERVAL_MS": "36000"},
            )
            environment = SimpleNamespace(
                env_paths=SimpleNamespace(agent_dir="/logs/agent"),
                agent_process_env=lambda env: env,
                exec=AsyncMock(return_value=SimpleNamespace(return_code=0)),
            )
            with patch("talos_agent.resolve_model", return_value=object()), patch("talos_agent.populate_usage"):
                agent = AgentFactory.create_agent_from_config(config, logs)
                await agent.run("Implement the task.", environment, None)

            environment.exec.assert_awaited_once()
            self.assertIn("--include-content", environment.exec.await_args.args[0])
            env = environment.exec.await_args.kwargs["env"]
            self.assertEqual(env["TALOS_BENCH_OPENAI_REQUEST_INTERVAL_MS"], "36000")
            self.assertEqual(env["GIT_AUTHOR_NAME"], "Talos")
