"""Run the Talos CLI inside native Harbor task environments."""

import os
from pathlib import Path

from harbor.agents.installed.base import BaseInstalledAgent
try:
    from harbor.agents.capabilities import AgentCapabilities
except ModuleNotFoundError:
    AgentCapabilities = None  # Harbor 0.22 predates the capability declaration.

from atif import export_atif
from payload import command, populate_usage, resolve_model, upload_payload


class TalosAgent(BaseInstalledAgent):
    include_content = False

    def __init__(self, *, product=None, model_name=None, talos_model=None, talos_config=None,
                 talos_dotenv=None, fixture_file=None, capture=None, mcp_config=None,
                 include_content=False, **kwargs):
        super().__init__(model_name=model_name or (talos_model and talos_model.replace(":", "/", 1)), **kwargs)
        self.product = Path(product or os.environ["TALOS_BENCH_PRODUCT"])
        self.connection = resolve_model(self.product, model_name, talos_model, talos_config, talos_dotenv)
        if not self.connection and not fixture_file:
            raise ValueError("Select a model with -m provider/model or supply a controlled fixture")
        self.fixture = Path(fixture_file) if not self.connection else None
        self.capture = capture
        self.mcp_config = Path(mcp_config) if mcp_config else None
        self.include_content = self.include_content or include_content

    @staticmethod
    def name():
        return "talos"

    async def install(self, environment):
        native = os.environ.get("TALOS_BENCH_PLATFORM")
        if native:
            machine = {"linux/arm64": "aarch64", "linux/amd64": "x86_64"}[native]
            await self.exec_as_root(environment, f'test "$(uname -m)" = {machine}')
        await self.exec_as_root(
            environment,
            '(command -v python3 >/dev/null && test -s /etc/ssl/certs/ca-certificates.crt || '
            '(apt-get -o Acquire::Retries=3 update && '
            'apt-get -o Acquire::Retries=3 install -y --no-install-recommends python3 ca-certificates))',
        )
        await upload_payload(environment, self.product, connection=self.connection, fixture=self.fixture,
                             mcp_config=self.mcp_config)

    async def run(self, instruction, environment, context):
        mode = "real" if self.connection else "fixture"
        await self.exec_as_agent(
            environment,
            command(self.environment_logs_dir, mode, instruction, include_content=self.include_content,
                    capture=self.capture),
        )

    def populate_context_post_run(self, context):
        populate_usage(self.logs_dir, context)


class AtifTalosAgent(TalosAgent):
    if AgentCapabilities is not None:
        capabilities = AgentCapabilities(atif=True)
    include_content = True

    def populate_context_post_run(self, context):
        super().populate_context_post_run(context)
        run = self.logs_dir / "talos-run.json"
        if run.is_file() and (self.logs_dir / "talos-trajectory.json").is_file():
            export_atif(self.logs_dir)
