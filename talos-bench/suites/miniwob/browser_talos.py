"""Run MiniWoB++ through the packaged Talos Browser MCP."""

from pathlib import Path
import shlex

from harbor_talos import AtifTalosAgent
from payload import REMOTE


class BrowserTalosAgent(AtifTalosAgent):
    async def install(self, environment):
        await super().install(environment)
        await environment.upload_file(Path(__file__).with_name("browser_trial.py"), f"{REMOTE}/browser_trial.py")
        result = await self.exec_as_root(
            environment, "install -d -o talos -g talos /logs/artifacts && "
            f"cp -a {REMOTE}/conf/migrations /opt/talos/conf/ && "
            f"chown -R talos:talos {REMOTE} {shlex.quote(str(self.environment_logs_dir))}",
        )
        if result.return_code:
            raise RuntimeError("Could not give the packaged Talos user its benchmark files")

    async def run(self, instruction, environment, context):
        mode = "real" if self.connection else "fixture"
        command = shlex.join([
            "/opt/talos/browser/venv/bin/python", f"{REMOTE}/browser_trial.py",
            "--include-content", "--", str(self.environment_logs_dir), mode, instruction,
        ])
        command += " >" + shlex.quote(f"{self.environment_logs_dir}/browser-controller.log") + " 2>&1"
        result = await environment.exec(command, user="talos", cwd="/home/talos")
        if result.return_code:
            raise RuntimeError("MiniWoB++ did not complete the Talos trial")
