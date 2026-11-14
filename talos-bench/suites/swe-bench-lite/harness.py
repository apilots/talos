"""Pinned SWE-bench case inputs and native Docker image preparation."""

from contextlib import closing, contextmanager
import json
from pathlib import Path
import shutil
import subprocess
from urllib.request import urlopen

import docker
import yaml

SUITE = Path(__file__).resolve().parent
DATASET, = json.loads((SUITE / "registry.json").read_text())
TASKS = {task["name"]: task for task in DATASET["tasks"]}
ENVIRONMENT_CASE = "sympy__sympy-20590"
RECIPES = SUITE / "cases" / ENVIRONMENT_CASE


def fetch_case(entry, *, include_gold=False):
    repo = entry["git_url"].removeprefix("https://github.com/").removesuffix(".git")
    base = f"https://raw.githubusercontent.com/{repo}/{entry['git_commit_id']}/{entry['path']}/"

    def read(name):
        with urlopen(base + name, timeout=30) as response:
            return response.read().decode()

    row = yaml.safe_load(read("task.yaml"))
    if (
        row["instance_id"] != entry["name"]
        or row["base_commit"] != entry["base_commit"]
        or row["repo"] != "sympy/sympy"
        or row["version"] != "1.7"
        or "SWE-bench/SWE-bench_Lite" not in row["datasets"]
        or row["image"].removesuffix(":latest") != entry["amd64_image"].split("@", 1)[0]
    ):
        raise RuntimeError(f"The pinned task does not match the selected SWE-bench Lite case {entry['name']}.")
    row = {
        **row,
        **json.loads(read("tests.json")),
        "eval_script": read("eval.sh"),
        "problem_statement": read("problem_statement.md"),
    }
    if include_gold:
        row["patch"] = read("gold.patch")
    return row


def build_image(context, builder, client, key, log, *, target_platform, arguments=None):
    command = ["docker", "buildx", "build", "--builder", builder, "--platform",
               target_platform, "--load", "--progress", "plain", "--tag", key]
    for name, value in (arguments or {}).items():
        command += ["--build-arg", f"{name}={value}"]
    print(f"Building {key}. Log: {log}", flush=True)
    with log.open("w") as stream:
        subprocess.run([*command, str(context)], stdout=stream,
                       stderr=subprocess.STDOUT, check=True)
    image = client.images.get(key)
    image_platform = f'{image.attrs["Os"]}/{image.attrs["Architecture"]}'
    if image_platform != target_platform:
        raise RuntimeError(f"Image {key} is {image_platform}, expected {target_platform}.")
    print(f"{key} {image_platform} {image.id}", flush=True)
    return {"tag": key, "id": image.id, "platform": image_platform}


def build_arm64_environment(output, builder, client):
    base = output / "base"
    environment = output / "environment"
    base.mkdir()
    environment.mkdir()
    shutil.copyfile(RECIPES / "base.Dockerfile", base / "Dockerfile")
    shutil.copyfile(RECIPES / "environment.Dockerfile", environment / "Dockerfile")
    shutil.copyfile(RECIPES / "setup_env.sh", environment / "setup_env.sh")
    base_image = build_image(
        base, builder, client, "talos-bench-swe-lite-base:arm64", output / "base.log",
        target_platform="linux/arm64",
    )
    revision = TASKS[ENVIRONMENT_CASE]["git_commit_id"]
    environment_image = build_image(
        environment, builder, client, f"talos-bench-swe-lite-sympy-env:{revision[:7]}-arm64",
        output / "environment.log", target_platform="linux/arm64",
        arguments={"SWE_BENCH_BASE": base_image["tag"]},
    )
    return {"base": base_image, "environment": environment_image}


@contextmanager
def native_docker(target_platform):
    builder = subprocess.check_output(["docker", "context", "show"], text=True).strip()
    daemon_id = subprocess.check_output(
        ["docker", "--context", builder, "info", "--format", "{{.ID}}"], text=True,
    ).strip()
    with closing(docker.from_env()) as client:
        if not daemon_id or client.info()["ID"] != daemon_id:
            raise RuntimeError("The Docker CLI context and Python SDK must use the same daemon.")
        server = client.version()
        if f'{server["Os"]}/{server["Arch"]}' != target_platform:
            raise RuntimeError(f"A native {target_platform} Docker server is required.")
        yield builder, client
