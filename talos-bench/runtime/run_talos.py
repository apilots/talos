"""Run Talos in the task directory with a fresh anonymous runtime."""

import argparse
from contextlib import nullcontext
import hashlib
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
WORKSPACE = Path.cwd()
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
    machine = platform.machine()
    elf_machine = {"aarch64": (183, "AArch64"), "x86_64": (62, "x86-64")}.get(machine)
    if (elf_machine is None or len(header) != 20 or header[:6] != b"\x7fELF\x02\x01"
            or struct.unpack_from("<H", header, 18)[0] != elf_machine[0]):
        raise RuntimeError(f"The Talos ELF binary must match the Linux task architecture ({machine})")
    logs.mkdir(parents=True, exist_ok=True)
    (logs / "architecture.txt").write_text(f"container={machine}\ntalos=ELF64 {elf_machine[1]}\n")

    provider, model = llm["provider"], llm["model"]
    connection = (llm.get("custom_providers") or llm)[provider]
    model_config = next(entry for entry in connection["models"] if entry["id"] == model)
    collaboration_tools = os.environ.get("TALOS_BENCH_COLLABORATION_TOOLS", "1")
    if collaboration_tools not in {"0", "1"}:
        raise ValueError("TALOS_BENCH_COLLABORATION_TOOLS must be 0 or 1")
    watch = os.environ.get("TALOS_BENCH_DELEGATION_WATCH") == "1" and collaboration_tools == "1"
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
                "kernel": {
                    "tools": {
                        "enable_collaboration_tools": collaboration_tools == "1",
                        "sandbox_policy": {
                            "user_permission_mode": "ask_for_approval",
                            "platform": {"linux_helper_executable": str(PRODUCT / "bin/talos-linux-sandbox-launcher")},
                        },
                    },
                    "plugins": {"sources": []},
                },
            },
            "llm": llm,
        }))
        environment = dict(os.environ, TALOS_HOME=home, TALOS_CONFIG=str(config),
                           TALOS_RESOURCE_DIR=str(PRODUCT), TALOS_TUI_NO_BROWSER="1")
        if (PRODUCT / "bin/rg").is_file():
            environment["PATH"] = f"{PRODUCT / 'bin'}{os.pathsep}{environment.get('PATH', os.defpath)}"
            probe = subprocess.run(["rg", "--version"], env=environment, capture_output=True,
                                   text=True, check=True)
            (logs / "ripgrep-version.txt").write_text(probe.stdout)
        guidance = PRODUCT / "workspace-AGENTS.md"
        if guidance.is_file():
            workspace = Path(home) / "workspace-talos/anonymous"
            workspace.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(guidance, workspace / "AGENTS.md")
            shutil.copyfile(guidance, logs / "workspace-AGENTS.md")
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
            command.extend(("--transcript", str(logs / "talos-transcript.json")))
        metadata = {
            "model": f"{provider}:{model}",
            "reasoning_effort": model_config.get("reasoning_effort"),
            "collaboration_tools": collaboration_tools == "1",
            "prices_per_million": model_config.get("cost"),
            "elapsed_seconds": None,
            "exit_code": None,
            "request_interval_ms": os.environ.get("TALOS_BENCH_OPENAI_REQUEST_INTERVAL_MS"),
            "delegation_watch": watch,
            "workspace_guidance_sha256": hashlib.sha256(guidance.read_bytes()).hexdigest() if guidance.is_file() else None,
            "version": subprocess.check_output([str(binary), "--version"], text=True).strip().removeprefix("talos "),
        }
        run_path = logs / "talos-run.json"
        run_path.write_text(json.dumps(metadata, indent=2) + "\n")
        started = time.monotonic()
        with (logs / "talos.stdout.txt").open("w") as stdout, (logs / "talos.stderr.txt").open("w") as stderr:
            command.extend(("--", options.instruction))
            if watch:
                from delegation_watch import wait, worktree_signature

                paths = list(filter(None, os.environ.get("TALOS_BENCH_WATCH_PATHS", "").split(":")))
                signature = worktree_signature(WORKSPACE, paths)
                with subprocess.Popen(command, env=environment, cwd=WORKSPACE, stdout=stdout, stderr=stderr) as process:
                    try:
                        exit_code = wait(process, home, logs, WORKSPACE, paths, signature)
                    finally:
                        if process.poll() is None:
                            process.kill()
            else:
                exit_code = subprocess.run(command, env=environment, cwd=WORKSPACE, stdout=stdout, stderr=stderr).returncode
        metadata.update(elapsed_seconds=time.monotonic() - started, exit_code=exit_code)
        final_metadata = run_path.with_suffix(".tmp")
        final_metadata.write_text(json.dumps(metadata, indent=2) + "\n")
        final_metadata.replace(run_path)
        if exit_code == 0 and options.capture:
            SUBMISSIONS.mkdir(parents=True, exist_ok=True)
            if baseline is not None:
                capture_patch(SUBMISSIONS / "submission.patch", baseline, Path(home) / "capture-index")
            else:
                shutil.copyfile(logs / "talos.stdout.txt", SUBMISSIONS / "answer.txt")
        return exit_code


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
