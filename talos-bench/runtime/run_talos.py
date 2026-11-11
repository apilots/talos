"""Run Talos in the task directory with a fresh anonymous runtime."""

import argparse
from contextlib import nullcontext
import json
import os
from pathlib import Path
import platform
import shutil
import struct
import subprocess
import tempfile
import time

PRODUCT = Path(__file__).resolve().parent
WORKSPACE = Path("/app")
SUBMISSIONS = Path("/logs/artifacts")


def capture_patch(destination, baseline, index):
    git = ["git", "-C", str(WORKSPACE)]
    environment = dict(os.environ, GIT_INDEX_FILE=str(index))
    subprocess.run([*git, "read-tree", baseline], env=environment, check=True)
    subprocess.run([*git, "add", "-A"], env=environment, check=True)
    with destination.open("wb") as patch:
        subprocess.run([*git, "diff", "--cached", "--binary", "--no-ext-diff", "--no-textconv",
                        baseline, "--"], env=environment, stdout=patch, check=True)


def run(options, llm, secret):
    logs = options.logs
    binary = PRODUCT / "bin/talos"
    with binary.open("rb") as executable:
        header = executable.read(20)
    # ELF64 little-endian EM_AARCH64.
    if platform.machine() != "aarch64" or header[:6] != b"\x7fELF\x02\x01" or struct.unpack_from("<H", header, 18)[0] != 183:
        raise RuntimeError("The task and the Talos binary must both be native ARM64")
    logs.mkdir(parents=True, exist_ok=True)
    (logs / "architecture.txt").write_text("container=aarch64\ntalos=ELF64 AArch64\n")

    provider, model = llm["provider"], llm["model"]
    with tempfile.TemporaryDirectory(prefix="talos-bench-") as home:
        if secret is not None:
            dotenv = Path(home) / ".env"
            shutil.move(secret, dotenv)
            dotenv.chmod(0o600)
        config = Path(home) / "conf/talos.yaml"
        config.parent.mkdir()
        config.write_text(json.dumps({
            "gateway": {"host": "127.0.0.1", "port": 8080, "auth": False},
            "workspaces": {"default_workspace_id": "default", "definitions": [
                {"workspace_id": "default", "root_prefix": "workspace-talos"},
            ]},
            "agent": {
                "name": "talos",
                "inner_model": {"provider": provider, "id": model},
                "kernel": {"tools": {"sandbox_policy": {
                    "user_permission_mode": "ask_for_approval",
                    "platform": {"linux_helper_executable": str(PRODUCT / "bin/talos-linux-sandbox-launcher")},
                }}, "plugins": {"sources": []}},
            },
            "llm": llm,
        }))
        environment = dict(os.environ, TALOS_HOME=home, TALOS_CONFIG=str(config),
                           TALOS_RESOURCE_DIR=str(PRODUCT), TALOS_TUI_NO_BROWSER="1")
        if (mcp_config := PRODUCT / "mcp.json").is_file():
            with (logs / "talos-mcp-install.log").open("w") as install_log:
                subprocess.run([str(binary), "plugin", "import-mcp", str(mcp_config)],
                               env=environment, cwd=WORKSPACE, check=True,
                               stdout=subprocess.DEVNULL, stderr=install_log)
        baseline = None
        if options.capture == "patch":
            baseline = subprocess.check_output(["git", "rev-parse", "HEAD"],
                                               cwd=WORKSPACE, text=True).strip()
        command = [str(binary), "exec", "--dangerously-skip-permissions",
                   "--model", f"{provider}:{model}", "--trajectory", str(logs / "talos-trajectory.json")]
        if options.include_content:
            command.append("--trajectory-include-content")
        started = time.monotonic()
        with (logs / "talos.stdout.txt").open("w") as stdout, (logs / "talos.stderr.txt").open("w") as stderr:
            result = subprocess.run(
                [*command, "--", options.instruction], env=environment, cwd=WORKSPACE,
                stdout=stdout, stderr=stderr, check=False,
            )
        (logs / "talos-run.json").write_text(json.dumps({
            "model": f"{provider}:{model}",
            "elapsed_seconds": time.monotonic() - started,
            "exit_code": result.returncode,
            "version": subprocess.check_output([str(binary), "--version"], text=True).strip().removeprefix("talos "),
        }, indent=2) + "\n")
        if result.returncode == 0 and options.capture:
            SUBMISSIONS.mkdir(parents=True, exist_ok=True)
            if baseline is not None:
                capture_patch(SUBMISSIONS / "submission.patch", baseline, Path(home) / "capture-index")
            else:
                shutil.copyfile(logs / "talos.stdout.txt", SUBMISSIONS / "answer.txt")
        return result.returncode


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("logs", type=Path)
    parser.add_argument("mode", choices=("fixture", "real"))
    parser.add_argument("instruction")
    parser.add_argument("--include-content", action="store_true")
    parser.add_argument("--capture", choices=("patch", "answer"))
    options = parser.parse_args()
    if options.mode == "fixture":
        from bench_fixture import model_config

        configuration = model_config()
        secret = None
    else:
        configuration = nullcontext(json.loads((PRODUCT / "model.json").read_text()))
        secret = PRODUCT / "model.env"
    with configuration as llm:
        exit_code = run(options, llm, secret)
    raise SystemExit(exit_code)
