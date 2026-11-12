# /// script
# requires-python = ">=3.12,<3.13"
# dependencies = ["docker==7.2.0", "PyYAML==6.0.3"]
# ///

"""Prepare a pinned Harbor task with native agent and verifier images."""

import argparse
import hashlib
import json
from pathlib import Path
import shutil

from harness import (
    BASE_COMMIT,
    CASE,
    RECIPES,
    TASK_REVISION,
    build_image,
    build_native_environment,
    fetch_case,
    native_docker,
)

SUITE = Path(__file__).resolve().parent
HARNESS_VERSION = "5.0.2"


def prepare(run, task_name, mode):
    if task_name != CASE:
        raise ValueError(f"Unsupported SWE-bench Lite case: {task_name}")
    row = fetch_case(include_gold=mode == "oracle")
    task = run / "task" / CASE
    agent = task / "environment"
    verifier = task / "tests"
    agent.mkdir(parents=True)
    verifier.mkdir()
    (task / "instruction.md").write_text(row["problem_statement"])
    shutil.copyfile(RECIPES / "agent.Dockerfile", agent / "Dockerfile")
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

    agent_tag = f"talos-bench-swe-lite-{CASE}:{TASK_REVISION[:7]}-arm64"
    verifier_tag = f"talos-bench-swe-lite-{CASE}-verifier:{TASK_REVISION[:7]}-{HARNESS_VERSION}-arm64"
    (task / "task.toml").write_text(f"""schema_version = "1.1"
artifacts = [{{ source = "/logs/artifacts/submission.patch", destination = "submission.patch" }}]

[task]
name = "swe-bench/{CASE}"
description = "Pinned SWE-bench Lite SymPy regression"

[metadata]
task_revision = "{TASK_REVISION}"
base_commit = "{BASE_COMMIT}"

[environment]
docker_image = "{agent_tag}"
build_timeout_sec = 900

[agent]
timeout_sec = 300

[verifier]
timeout_sec = 300

[verifier.environment]
docker_image = "{verifier_tag}"
network_mode = "no-network"
""")
    builds = run / "swe-bench-build"
    builds.mkdir()
    with native_docker() as (builder, client):
        images = build_native_environment(builds, builder, client)
        images["agent"] = build_image(
            agent, builder, client, agent_tag, builds / "agent.log",
            arguments={"SWE_BENCH_ENV": images["environment"]["tag"], "TASK_BASE_COMMIT": BASE_COMMIT},
        )
        images["verifier"] = build_image(
            verifier, builder, client, verifier_tag, builds / "verifier.log",
            arguments={"SWE_BENCH_AGENT_IMAGE": agent_tag, "SWEBENCH_VERSION": HARNESS_VERSION},
        )

    provenance = run / "provenance"
    provenance.mkdir()
    (provenance / f"{CASE}.json").write_text(json.dumps({
        "suite": "swe-bench-lite",
        "task": CASE,
        "taskRevision": TASK_REVISION,
        "baseCommit": BASE_COMMIT,
        "harness": f"swebench=={HARNESS_VERSION}",
        "images": images,
        "verifierFiles": {path.name: hashlib.sha256(path.read_bytes()).hexdigest()
                          for path in sorted(verifier.iterdir())},
    }, indent=2) + "\n")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("task_name")
    parser.add_argument("mode", choices=("fixture", "real", "oracle"))
    prepare(**vars(parser.parse_args()))
