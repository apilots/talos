"""Run Talos inside Pier's DeepSWE agent environment."""

import os
from pathlib import Path
from urllib.parse import urlsplit

from pier.agents.base import BaseAgent
from pier.models.agent.network import NetworkAllowlist

from payload import command, populate_usage, resolve_model, upload_payload


class TalosAgent(BaseAgent):
    def __init__(self, *, product=None, model_name=None, talos_model=None, talos_config=None,
                 talos_dotenv=None, extra_env=None, **kwargs):
        super().__init__(model_name=model_name or (talos_model and talos_model.replace(":", "/", 1)), **kwargs)
        self.product = Path(product or os.environ["TALOS_BENCH_PRODUCT"])
        self.connection = resolve_model(self.product, model_name, talos_model, talos_config, talos_dotenv)
        self.extra_env = extra_env or {}
        if not self.connection:
            raise ValueError("Select a model with -m provider/model")

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
            command(logs, "real", instruction, include_content=True),
            cwd="/app",
            env=environment.agent_process_env({**git_identity, **self.extra_env}),
        )
        if not (self.logs_dir / "talos.stderr.txt").exists():
            await environment.download_file(f"{logs}/talos.stderr.txt", self.logs_dir / "talos.stderr.txt")
        populate_usage(self.logs_dir, context)
        if result.return_code:
            raise RuntimeError(f"Talos CLI exited with code {result.return_code}")
