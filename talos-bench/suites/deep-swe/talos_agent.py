"""Run Talos inside Pier's DeepSWE agent environment."""

from pathlib import Path
from urllib.parse import urlsplit

from pier.agents.base import BaseAgent
from pier.models.agent.network import NetworkAllowlist

from payload import command, model_configuration, populate_usage, upload_payload


class TalosAgent(BaseAgent):
    def __init__(self, *, product, talos_model, talos_config, talos_dotenv, model_name=None, **kwargs):
        super().__init__(model_name=model_name or talos_model.replace(":", "/", 1), **kwargs)
        self.product = Path(product)
        self.connection = model_configuration(talos_model, talos_config, talos_dotenv)

    @staticmethod
    def name():
        return "talos"

    def version(self):
        return None

    def network_allowlist(self):
        llm, _ = self.connection
        provider = llm["provider"]
        connection = (llm.get("custom_providers") or llm)[provider]
        endpoint = urlsplit(connection.get("base_url") or "")
        if endpoint.scheme != "https" or not endpoint.hostname:
            raise ValueError("The Pier model connection requires an HTTPS base URL")
        return NetworkAllowlist(domains=[endpoint.hostname])

    async def setup(self, environment):
        await upload_payload(environment, self.product, connection=self.connection)

    async def run(self, instruction, environment, context):
        logs = environment.env_paths.agent_dir
        git_identity = {
            "GIT_AUTHOR_NAME": "Talos",
            "GIT_AUTHOR_EMAIL": "talos@local",
            "GIT_COMMITTER_NAME": "Talos",
            "GIT_COMMITTER_EMAIL": "talos@local",
        }
        result = await environment.exec(
            command(logs, "real", instruction),
            cwd="/app",
            env=environment.agent_process_env(git_identity),
        )
        if not (self.logs_dir / "talos.stderr.txt").exists():
            await environment.download_file(f"{logs}/talos.stderr.txt", self.logs_dir / "talos.stderr.txt")
        populate_usage(self.logs_dir, context)
        if result.return_code:
            raise RuntimeError(f"Talos CLI exited with code {result.return_code}")
