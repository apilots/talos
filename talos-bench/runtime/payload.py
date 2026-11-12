"""Talos product, model configuration, and evidence shared by benchmark runners."""

import json
from pathlib import Path
import re
import shlex
from tempfile import TemporaryDirectory

REMOTE = "/installed-agent/talos"
METRICS_PREFIX = "TALOS_EXEC_METRICS "


async def upload_payload(environment, product, *, connection=None, fixture=None, mcp_config=None):
    result = await environment.exec(f"mkdir -p {REMOTE}", user="root")
    if result.return_code:
        raise RuntimeError("Could not create the Talos installation directory")
    await environment.upload_dir(product, REMOTE)
    await environment.upload_file(Path(__file__).with_name("run_talos.py"), f"{REMOTE}/run_talos.py")
    if mcp_config is not None:
        await environment.upload_file(mcp_config, f"{REMOTE}/mcp.json")
    if fixture is not None:
        await environment.upload_file(Path(__file__).with_name("fixture.py"), f"{REMOTE}/fixture.py")
        await environment.upload_file(fixture, f"{REMOTE}/bench_fixture.py")
        return
    llm, credential = connection
    with TemporaryDirectory(prefix="talos-bench-model-") as directory:
        config = Path(directory) / "model.json"
        dotenv = Path(directory) / "model.env"
        config.write_text(json.dumps(llm))
        dotenv.touch(mode=0o600)
        dotenv.write_text(credential + "\n")
        await environment.upload_file(config, f"{REMOTE}/model.json")
        await environment.upload_file(dotenv, f"{REMOTE}/model.env")


def command(logs_dir, mode, instruction, *, include_content=False, capture=None):
    args = ["python3", f"{REMOTE}/run_talos.py"]
    if include_content:
        args.append("--include-content")
    if capture:
        args.extend(("--capture", capture))
    return shlex.join([*args, "--", str(logs_dir), mode, instruction])


def model_configuration(selected_model, config, dotenv):
    import yaml

    provider, separator, model = selected_model.partition(":")
    if not separator or not provider or not model:
        raise ValueError("Select a Talos model as provider:model")
    configured = yaml.safe_load(Path(config).read_text())["llm"]
    llm = {"provider": provider, "model": model}
    if provider in configured.get("custom_providers", {}):
        connection = configured["custom_providers"][provider]
        llm["custom_providers"] = {provider: connection}
    else:
        connection = configured.get(provider)
        if not isinstance(connection, dict):
            raise ValueError(f"Provider {provider} is missing from the Talos configuration")
        llm[provider] = connection
    if not any(entry["id"] == model for entry in connection["models"]):
        raise ValueError(f"Model {selected_model} is missing from the Talos configuration")
    placeholder = re.fullmatch(r"\$\{(TALOS_[A-Z0-9_]+)\}", connection.get("api_key") or "")
    if placeholder is None:
        raise ValueError(f"Provider {provider} needs a Talos dotenv reference for its API key")
    key = placeholder[1]
    # Transfer only the matching assignment; Talos parses its value.
    credential = next((line for line in reversed(Path(dotenv).read_text().splitlines())
                       if re.fullmatch(rf"\s*(?:export )?{key}\s*=.+", line)), None)
    if credential is None:
        raise ValueError(f"Missing {key} in the supplied Talos dotenv")
    return llm, credential



def cli_log_lines(logs_dir):
    stderr = logs_dir / "talos.stderr.txt"
    return stderr.read_text().splitlines() if stderr.is_file() else []


def run_metrics(lines):
    for line in reversed(lines):
        if line.startswith(METRICS_PREFIX):
            return json.loads(line[len(METRICS_PREFIX):])
    return None


def populate_usage(logs_dir, context):
    metrics = run_metrics(cli_log_lines(logs_dir))
    if metrics is not None:
        usage = metrics["usage"]
        context.n_input_tokens = usage["input_tokens"]
        context.n_output_tokens = usage["output_tokens"]
        context.n_cache_tokens = usage["cache_read_input_tokens"]
