"""Pinned SWE-bench case inputs and native Docker image preparation."""

from contextlib import closing, contextmanager
import json
from pathlib import Path
import platform
import shutil
import subprocess
from urllib.request import urlopen

import docker
import yaml

CASE = "sympy__sympy-20590"
DATASET, = json.loads(Path(__file__).with_name("registry.json").read_text())
ENTRY = next(task for task in DATASET["tasks"] if task["name"] == CASE)
TASK_REVISION = ENTRY["git_commit_id"]
BASE_COMMIT = "cffd4e0f86fefd4802349a9f9b19ed70934ea354"
RECIPES = Path(__file__).resolve().parent / "cases" / CASE


def fetch_case(*, include_gold=False):
    repo = ENTRY["git_url"].removeprefix("https://github.com/").removesuffix(".git")
    base = f"https://raw.githubusercontent.com/{repo}/{TASK_REVISION}/{ENTRY['path']}/"

    def read(name):
        with urlopen(base + name, timeout=30) as response:
            return response.read().decode()

    row = yaml.safe_load(read("task.yaml"))
    if (
        row["instance_id"] != CASE
        or row["base_commit"] != BASE_COMMIT
        or "SWE-bench/SWE-bench_Lite" not in row["datasets"]
    ):
        raise RuntimeError("The pinned task does not match the selected SWE-bench Lite case.")
    row = {
        **row,
        **json.loads(read("tests.json")),
        "eval_script": read("eval.sh"),
        "problem_statement": read("problem_statement.md"),
    }
    if include_gold:
        row["patch"] = read("gold.patch")
    return row


def build_image(context, builder, client, key, log, *, arguments=None):
    command = ["docker", "buildx", "build", "--builder", builder, "--platform",
               "linux/arm64", "--load", "--progress", "plain", "--tag", key]
    for name, value in (arguments or {}).items():
        command += ["--build-arg", f"{name}={value}"]
    print(f"Building {key}. Log: {log}", flush=True)
    with log.open("w") as stream:
        subprocess.run([*command, str(context)], stdout=stream,
                       stderr=subprocess.STDOUT, check=True)
    image = client.images.get(key)
    if (image.attrs["Os"], image.attrs["Architecture"]) != ("linux", "arm64"):
        raise RuntimeError(f"Image {key} is not linux/arm64.")
    print(f"{key} linux/arm64 {image.id}", flush=True)
    return {"tag": key, "id": image.id, "platform": "linux/arm64"}


def build_native_environment(output, builder, client):
    base = output / "base"
    environment = output / "environment"
    base.mkdir()
    environment.mkdir()
    shutil.copyfile(RECIPES / "base.Dockerfile", base / "Dockerfile")
    shutil.copyfile(RECIPES / "environment.Dockerfile", environment / "Dockerfile")
    shutil.copyfile(RECIPES / "setup_env.sh", environment / "setup_env.sh")
    base_image = build_image(
        base, builder, client, "talos-bench-swe-lite-base:arm64", output / "base.log",
    )
    environment_image = build_image(
        environment, builder, client, f"talos-bench-swe-lite-sympy-env:{TASK_REVISION[:7]}-arm64",
        output / "environment.log", arguments={"SWE_BENCH_BASE": base_image["tag"]},
    )
    return {"base": base_image, "environment": environment_image}


@contextmanager
def native_docker():
    if platform.machine() not in {"arm64", "aarch64"}:
        raise RuntimeError("An ARM64 host is required.")
    builder = subprocess.check_output(["docker", "context", "show"], text=True).strip()
    daemon_id = subprocess.check_output(
        ["docker", "--context", builder, "info", "--format", "{{.ID}}"], text=True,
    ).strip()
    with closing(docker.from_env()) as client:
        if not daemon_id or client.info()["ID"] != daemon_id:
            raise RuntimeError("The Docker CLI context and Python SDK must use the same daemon.")
        server = client.version()
        if (server["Os"], server["Arch"]) != ("linux", "arm64"):
            raise RuntimeError("A linux/arm64 Docker server is required.")
        yield builder, client
