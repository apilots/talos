# /// script
# requires-python = ">=3.12,<3.13"
# dependencies = ["docker==7.2.0", "PyYAML==6.0.3"]
# ///

"""Prepare pinned Harbor tasks with native agent and verifier images."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil

from harness import (
    RECIPES,
    TASKS,
    build_image,
    build_arm64_environment,
    fetch_case,
    native_docker,
)

SUITE = Path(__file__).resolve().parent
HARNESS_VERSION = "5.0.2"


def prepare_task(run, entry, mode, target_platform, builder, client, environment_images):
    task_name = entry["name"]
    revision = entry["git_commit_id"]
    base_commit = entry["base_commit"]
    arch = target_platform.removeprefix("linux/")
    row = fetch_case(entry, include_gold=mode == "oracle")
    task = run / "task" / task_name
    agent = task / "environment"
    verifier = task / "tests"
    agent.mkdir(parents=True)
    verifier.mkdir()
    (task / "instruction.md").write_text(row["problem_statement"])
    agent_recipe = "agent.Dockerfile" if arch == "arm64" else "agent-amd64.Dockerfile"
    shutil.copyfile(RECIPES / agent_recipe, agent / "Dockerfile")
    shutil.copyfile(RECIPES / "verifier.Dockerfile", verifier / "Dockerfile")
    for name in ("test.sh", "grade.py"):
        shutil.copyfile(SUITE / name, verifier / name)
    grading_inputs = {key: value for key, value in row.items() if key != "patch"}
    (verifier / "case.json").write_text(json.dumps(grading_inputs, indent=2) + "\n")
    if mode == "oracle":
        solution = task / "solution"
        solution.mkdir()
        (solution / "gold.patch").write_text(row["patch"])
        (solution / "solve.sh").write_text(
            "#!/bin/bash\nset -euo pipefail\n"
            "cp /solution/gold.patch /logs/artifacts/submission.patch\n"
        )

    agent_tag = f"talos-bench-swe-lite-{task_name}:{revision[:7]}-{arch}"
    verifier_tag = f"talos-bench-swe-lite-{task_name}-verifier:{revision[:7]}-{HARNESS_VERSION}-{arch}"
    agent_timeout = 900 if mode == "real" else 300
    (task / "task.toml").write_text(f"""schema_version = "1.1"
artifacts = [{{ source = "/logs/artifacts/submission.patch", destination = "submission.patch" }}]

[task]
name = "swe-bench/{task_name}"
description = "Pinned SWE-bench Lite SymPy regression"

[metadata]
task_revision = "{revision}"
base_commit = "{base_commit}"

[environment]
docker_image = "{agent_tag}"
build_timeout_sec = 900

[agent]
timeout_sec = {agent_timeout}

[verifier]
timeout_sec = 300

[verifier.environment]
docker_image = "{verifier_tag}"
network_mode = "no-network"
""")
    builds = run / "swe-bench-build" / task_name
    builds.mkdir()
    images = dict(environment_images)
    agent_base = images["environment"]["tag"] if arch == "arm64" else entry["amd64_image"]
    images["agent"] = build_image(
        agent, builder, client, agent_tag, builds / "agent.log",
        target_platform=target_platform,
        arguments={"SWE_BENCH_ENV": agent_base, "TASK_BASE_COMMIT": base_commit},
    )
    images["verifier"] = build_image(
        verifier, builder, client, verifier_tag, builds / "verifier.log",
        target_platform=target_platform,
        arguments={"SWE_BENCH_AGENT_IMAGE": agent_tag, "SWEBENCH_VERSION": HARNESS_VERSION},
    )

    provenance = run / "provenance"
    provenance.mkdir(exist_ok=True)
    (provenance / f"{task_name}.json").write_text(json.dumps({
        "suite": "swe-bench-lite",
        "task": task_name,
        "taskRevision": revision,
        "baseCommit": base_commit,
        "harness": f"swebench=={HARNESS_VERSION}",
        "agentBaseImage": agent_base,
        "images": images,
        "verifierFiles": {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                          for path in sorted(verifier.iterdir())},
    }, indent=2) + "\n")


def prepare(run, selection, mode):
    target_platform = os.environ["TALOS_BENCH_PLATFORM"]
    if target_platform not in {"linux/arm64", "linux/amd64"}:
        raise ValueError(f"Unsupported SWE-bench Lite Docker platform: {target_platform}")
    builds = run / "swe-bench-build"
    builds.mkdir()
    with native_docker(target_platform) as (builder, client):
        environment = build_arm64_environment(builds, builder, client) if target_platform == "linux/arm64" else {}
        for task_name in selection.splitlines():
            prepare_task(run, TASKS[task_name], mode, target_platform, builder, client, environment)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("selection")
    parser.add_argument("mode", choices=("fixture", "real", "oracle"))
    prepare(**vars(parser.parse_args()))
