"""Exercise the fixed-release Arena adapter through Harbor and a controlled model."""

import argparse
import asyncio
import json
from pathlib import Path
import shutil
from tempfile import TemporaryDirectory
import uuid

from harbor.environments.docker.docker import DockerEnvironment
from harbor.models.agent.context import AgentContext
from harbor.models.task.config import EnvironmentConfig
from harbor.models.trial.paths import TrialPaths
from harbor.utils.trajectory_validator import TrajectoryValidator

from arena_agent import CustomAgent, PRODUCTS, SOURCE, VERSION

ROOT = Path(__file__).resolve().parents[2]
MODEL_SERVER = '''from pathlib import Path
import signal
from fixture import model_config, tool_responses

def respond(request):
    assert request.get("reasoning_effort") == "low", "Reasoning effort did not reach the provider"
    if results := tool_responses(request):
        assert "ARENA_SMOKE_TOOL_OK" in results[-1]["content"]
        return "ARENA_SMOKE_DONE"
    return "Bash", {"command": "printf ARENA_SMOKE_TOOL_OK > /tmp/arena-smoke-result && cat /tmp/arena-smoke-result"}

with model_config("arena-controlled", respond) as model:
    Path("/tmp/arena-smoke-endpoint").write_text(model["custom_providers"]["fixture"]["base_url"])
    signal.pause()
'''


async def smoke(output):
    output.mkdir(parents=True)
    paths = TrialPaths(output / "trial")
    paths.mkdir()
    with TemporaryDirectory(prefix="arena-smoke-") as temporary:
        directory = Path(temporary)
        shutil.copyfile(ROOT / "talos-e2e/docker/tui-install-ubuntu-24.04.Dockerfile",
                        directory / "Dockerfile")
        server = directory / "model_server.py"
        server.write_text(MODEL_SERVER)
        environment = DockerEnvironment(
            environment_dir=directory, environment_name="talos-arena-smoke",
            session_id=uuid.uuid4().hex, trial_paths=paths,
            task_env_config=EnvironmentConfig(workdir="/workspace"))
        try:
            async with asyncio.timeout(360):
                await environment.start(force_build=False)
                await environment.upload_file(Path(__file__).with_name("fixture.py"), "/tmp/fixture.py")
                await environment.upload_file(server, "/tmp/model_server.py")
                result = await environment.exec(
                    "python3 /tmp/model_server.py >/tmp/arena-model.log 2>&1 </dev/null & "
                    "timeout 15 sh -c 'until test -s /tmp/arena-smoke-endpoint; do sleep 0.1; done; "
                    "cat /tmp/arena-smoke-endpoint'")
                if result.return_code:
                    raise RuntimeError("Controlled model did not become ready")
                agent = CustomAgent(
                    logs_dir=paths.agent_dir, model_name="arena-controlled",
                    base_url=result.stdout.strip(), model_api_format="openai_chat_completions",
                    model_api_key_name="ARENA_FIXTURE_KEY", extra_env={"ARENA_FIXTURE_KEY": "fixture"},
                    reasoning_effort="low")
                context = AgentContext()
                await agent.setup(environment)
                try:
                    await agent.run("Run the controlled Bash tool and return its result.", environment, context)
                finally:
                    await environment.download_file("/tmp/arena-model.log", output / "model-server.log")
                result = await environment.exec("cat /tmp/arena-smoke-result")
                assert result.return_code == 0 and result.stdout == "ARENA_SMOKE_TOOL_OK", result
                trace = json.loads((paths.agent_dir / "trajectory.json").read_text())
                assert TrajectoryValidator().validate(trace), "Invalid ATIF"
                calls = [step for step in trace["steps"] if step.get("tool_calls")]
                assert len(calls) == 1, calls
                assert calls[0]["observation"]["results"][0]["source_call_id"] == calls[0]["tool_calls"][0]["tool_call_id"]
                assert "ARENA_SMOKE_DONE" in (paths.agent_dir / "talos.stdout.txt").read_text()
                assert context.n_input_tokens == 200 and context.n_output_tokens == 20, context
                provenance = json.loads((paths.agent_dir / "arena-provenance.json").read_text())
                assert provenance["version"] == VERSION and provenance["source"] == SOURCE, provenance
                assert provenance["product_sha256"] == PRODUCTS[provenance["machine"]][1], provenance
                summary = {"status": "passed", "controlled_model": True, "release": VERSION, "evidence_dir": str(output),
                           "reasoning_effort": "low", "steps": len(trace["steps"]),
                           "context": context.model_dump(), "provenance": provenance}
                (output / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
                print(json.dumps(summary))
        finally:
            async with asyncio.timeout(60):
                await environment.stop(delete=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path,
                        default=ROOT / "output/arena/hosted-smoke" / uuid.uuid4().hex,
                        help="New evidence directory; must not already exist")
    asyncio.run(smoke(parser.parse_args().output.resolve()))
