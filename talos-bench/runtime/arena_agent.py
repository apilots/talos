"""Standalone Runta Arena entry point using the released Talos Bench runtime."""

import asyncio
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import shlex
from tempfile import TemporaryDirectory
from urllib.request import urlopen

from harbor.agents.agent_auth_resolution import ModelConnectionSpec, ProviderAccess
from harbor.agents.base import BaseAgent
from harbor.agents.capabilities import AgentCapabilities
from harbor.models.model_api_format import ModelApiFormat


VERSION = "0.1.19"
SOURCE = "d8ff8a7278f482602ce35dbad77ee1e122c17996"
RELEASE_ROOT = f"https://github.com/apilots/talos/releases/download/v{VERSION}"
SOURCE_ROOT = f"https://raw.githubusercontent.com/apilots/talos/{SOURCE}/talos-bench/runtime"
PRODUCTS = {
    "x86_64": ("amd64", "9d5f01a8a2188c9ae57d87be63c87962843fc9efef028b31905939c4b8ac554a"),
    "aarch64": ("arm64", "79db86dbf116583b04fe142bd44e2c45648d8e2ef029b3d5ef218140237a5a61"),
}
RUNTIME_FILES = {
    "payload.py": "558b49c06b2525bb1e8b6d1f3a8a107b285399328950c561bbf158f0538f7f97",
    "run_talos.py": "2a8c904fc5dc77ba547d9f11628fe43c2b2bfa21095c6c5c96ed078a36a95ab9",
    "atif.py": "3b7152af2ca6228d4dd7476db0f97bfc57d16a95813075d7226ee387b2bc1e0f",
    "delegation_watch.py": "0efeef5f6ad066bb04c355188a4cbe335af6c038355eeb967e235b333a3ba4e6",
}


def download(url, destination, expected_sha256, limit):
    """Download a bounded artifact and verify its pinned checksum before use."""
    digest = hashlib.sha256()
    size = 0
    with urlopen(url, timeout=60) as response, destination.open("wb") as output:
        while chunk := response.read(1024 * 1024):
            size += len(chunk)
            if size > limit:
                raise ValueError("Talos artifact exceeds the download limit")
            digest.update(chunk)
            output.write(chunk)
    if digest.hexdigest() != expected_sha256:
        raise ValueError("Talos artifact checksum mismatch")


def prepare_runtime(directory):
    for name, checksum in RUNTIME_FILES.items():
        download(f"{SOURCE_ROOT}/{name}", directory / name, checksum, 128 * 1024)
    return directory


def installation_command(machine, remote):
    architecture, checksum = PRODUCTS[machine]
    url = f"{RELEASE_ROOT}/Talos-TUI-{VERSION}-linux-{architecture}.tar.gz"
    script = f"""import hashlib
from pathlib import Path
from urllib.request import urlopen
with urlopen({url!r}, timeout=60) as response:
    data = response.read(64 * 1024 * 1024 + 1)
if len(data) > 64 * 1024 * 1024:
    raise ValueError("Talos artifact exceeds the download limit")
if hashlib.sha256(data).hexdigest() != {checksum!r}:
    raise ValueError("Talos artifact checksum mismatch")
Path({(remote + '/product.tar.gz')!r}).write_bytes(data)
"""
    return (f"mkdir -p {remote} && python3 -c {shlex.quote(script)} && "
            f"tar -xzf {remote}/product.tar.gz -C {remote} && "
            f"rm {remote}/product.tar.gz && {remote}/bin/talos --version")


def load_payload(product):
    spec = importlib.util.spec_from_file_location("talos_arena_payload", product / "payload.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def talos_connection(connection, effort):
    formats = {ModelApiFormat.OPENAI_CHAT_COMPLETIONS: ("chat", "max_tokens"),
               ModelApiFormat.OPENAI_RESPONSES: ("responses", "max_output_tokens")}
    if connection.model_api_format not in formats:
        raise ValueError("Talos Arena supports OpenAI Chat Completions and Responses")
    if not connection.model_id or not connection.api_key or not connection.execution_base_url:
        raise ValueError("Talos Arena requires a model, API credential and endpoint")
    protocol, limit = formats[connection.model_api_format]
    model = {"id": connection.model_id, "name": connection.model_id,
             "context_window": 128000, "input": ["text"],
             "request_parameters": {limit: 131072}}
    if effort is not None:
        efforts = {"low": "light", "medium": "medium", "high": "high", "xhigh": "xhigh", "max": "max"}
        if effort not in efforts:
            raise ValueError("Talos Arena supports low, medium, high, xhigh and max reasoning effort")
        model.update(reasoning=True, reasoning_effort=efforts[effort],
                     reasoning_efforts=[efforts[effort]])
    llm = {"provider": "arena", "model": connection.model_id, "custom_providers": {"arena": {
        "name": "Runta Arena",
        "api_key": "${TALOS_LLM_OPENAI_API_KEY}",
        "base_url": connection.execution_base_url,
        "protocol": protocol, "models": [model]}}}
    quoted = json.dumps(connection.api_key).replace("$", r"\$")
    return llm, f"TALOS_LLM_OPENAI_API_KEY={quoted}"


class CustomAgent(BaseAgent):
    capabilities = AgentCapabilities(base_url=True, reasoning_effort=True, atif=True)
    MODEL_CONNECTION = ModelConnectionSpec(
        default_provider="openai",
        inject_default_base_url=True,
        agent_provider_access=ProviderAccess(
            api_key_candidate_names=["OPENAI_API_KEY"],
            base_url_candidate_names=["OPENAI_BASE_URL", "OPENAI_API_BASE"]),
        supported_api_formats=[ModelApiFormat.OPENAI_CHAT_COMPLETIONS, ModelApiFormat.OPENAI_RESPONSES],
        default_api_format_by_provider={"default": ModelApiFormat.OPENAI_CHAT_COMPLETIONS},
    )

    def __init__(self, *args, reasoning_effort=None, **kwargs):
        if reasoning_effort is None:
            reasoning_effort = "max"
        super().__init__(*args, reasoning_effort=reasoning_effort, **kwargs)
        self.effort = reasoning_effort
        self._directory = None
        self.product = None
        self.payload = None

    @staticmethod
    def name():
        return "talos"

    def version(self):
        return VERSION

    async def setup(self, environment):
        self.logs_dir.mkdir(parents=True, exist_ok=True)
        result = await environment.exec("uname -m")
        machine = (result.stdout or "").strip()
        if result.return_code or machine not in PRODUCTS:
            raise RuntimeError("Talos Arena requires native Linux amd64 or arm64")
        self._directory = TemporaryDirectory(prefix="talos-arena-")
        self.product = await asyncio.to_thread(prepare_runtime, Path(self._directory.name))
        self.payload = load_payload(self.product)
        connection = talos_connection(self.model_connection, self.effort)
        result = await environment.exec(
            "command -v python3 >/dev/null && test -s /etc/ssl/certs/ca-certificates.crt",
            user="root")
        if result.return_code:
            result = await environment.exec(
                "apt-get update && apt-get install -y --no-install-recommends python3 ca-certificates",
                user="root")
            if result.return_code:
                raise RuntimeError("Talos task dependencies could not be installed")
        result = await environment.exec(installation_command(machine, self.payload.REMOTE),
                                        user="root", timeout_sec=180)
        (self.logs_dir / "installation.log").write_text((result.stdout or "") + (result.stderr or ""))
        if result.return_code or (result.stdout or "").strip() != f"talos {VERSION}":
            raise RuntimeError("Installed Talos version does not match the pinned release")
        for name in ("run_talos.py", "delegation_watch.py"):
            await environment.upload_file(self.product / name, f"{self.payload.REMOTE}/{name}")
        llm, credential = connection
        config = self.product / "model.json"
        dotenv = self.product / "model.env"
        config.write_text(json.dumps(llm))
        dotenv.touch(mode=0o600)
        dotenv.write_text(credential + "\n")
        await environment.upload_file(config, f"{self.payload.REMOTE}/model.json")
        await environment.upload_file(dotenv, f"{self.payload.REMOTE}/model.env")
        dotenv.unlink()
        (self.logs_dir / "arena-provenance.json").write_text(json.dumps({
            "version": VERSION, "source": SOURCE, "machine": machine,
            "product_sha256": PRODUCTS[machine][1], "runtime_sha256": RUNTIME_FILES,
            "model": self.model_connection.model_id,
            "api_format": str(self.model_connection.model_api_format),
            "reasoning_effort": self.effort,
            "max_output_tokens": 131072,
        }, indent=2) + "\n")

    async def run(self, instruction, environment, context):
        failure = None
        try:
            command = self.payload.command(self.environment_logs_dir, "real", instruction,
                                           include_content=True)
            result = await environment.exec(command)
            if result.return_code:
                raise RuntimeError(f"Talos exited with code {result.return_code}; inspect agent logs")
        except BaseException as error:
            failure = error
            raise
        finally:
            try:
                await self._collect(environment, context)
            except Exception:
                if failure is None:
                    raise
                self.logger.exception("Talos evidence collection failed after execution failure")
            finally:
                if self._directory is not None:
                    self._directory.cleanup()

    async def _collect(self, environment, context):
        async with asyncio.timeout(60):
            await environment.download_dir(str(self.environment_logs_dir), self.logs_dir)
        self.payload.populate_usage(self.logs_dir, context)
        export = "from pathlib import Path; from atif import export_atif; import sys; export_atif(Path(sys.argv[1]))"
        process = await asyncio.create_subprocess_exec(
            sys.executable, "-c", export, str(self.logs_dir.resolve()),
            cwd=self.product, stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE)
        try:
            async with asyncio.timeout(30):
                _, stderr = await process.communicate()
        except BaseException:
            if process.returncode is None:
                process.kill()
            await process.wait()
            raise
        if process.returncode:
            (self.logs_dir / "atif-export.stderr.txt").write_bytes(stderr)
            raise RuntimeError("Talos ATIF export failed; inspect atif-export.stderr.txt")
