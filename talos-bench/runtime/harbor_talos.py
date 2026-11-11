"""Run the Talos CLI inside native ARM64 Harbor task environments."""

from pathlib import Path

from harbor.agents.capabilities import AgentCapabilities
from harbor.agents.installed.base import BaseInstalledAgent

from atif import export_atif
from payload import command, model_configuration, populate_usage, upload_payload


class TalosAgent(BaseInstalledAgent):
    include_content = False

    def __init__(self, *, product, talos_model=None, talos_config=None, talos_dotenv=None,
                 fixture_file=None, capture=None, mcp_config=None, **kwargs):
        super().__init__(**kwargs)
        self.product = Path(product)
        self.connection = model_configuration(talos_model, talos_config, talos_dotenv) if talos_model else None
        self.fixture = Path(fixture_file) if not talos_model else None
        self.capture = capture
        self.mcp_config = Path(mcp_config) if mcp_config else None

    @staticmethod
    def name():
        return "talos"

    async def install(self, environment):
        await self.exec_as_root(
            environment,
            'test "$(uname -m)" = aarch64 && '
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
            cwd="/app",
        )

    def populate_context_post_run(self, context):
        populate_usage(self.logs_dir, context)


class AtifTalosAgent(TalosAgent):
    capabilities = AgentCapabilities(atif=True)
    include_content = True

    def populate_context_post_run(self, context):
        super().populate_context_post_run(context)
        run = self.logs_dir / "talos-run.json"
        if run.is_file() and (self.logs_dir / "talos-trajectory.json").is_file():
            export_atif(self.logs_dir)
