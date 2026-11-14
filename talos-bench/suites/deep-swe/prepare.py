"""Prepare native agent and verifier images for a pinned DeepSWE case."""

import argparse
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
from tempfile import TemporaryDirectory
import tomllib

from summarize import task_entries, write_json


MARS_BASE = "public.ecr.aws/x8v8d7g8/mars-base"
ARM64_BASE = f"{MARS_BASE}@sha256:8d4d973c993776850e6249f909431040f0f0e694162768b1ed1409ba64162cb2"


def checked(*command):
    subprocess.run(command, check=True)


def replace_once(text, pattern, value):
    rewritten, matches = re.subn(pattern, lambda match: value, text, flags=re.MULTILINE)
    if matches != 1:
        raise ValueError(f"Expected one match for {pattern}; found {matches}")
    return rewritten


def inspect_image(tag, expected_platform):
    output = subprocess.check_output(
        ["docker", "image", "inspect", "--format", "{{.Os}}/{{.Architecture}} {{.Id}}", tag],
        text=True,
    ).strip()
    platform, image_id = output.split()
    if platform != expected_platform:
        raise ValueError(f"Expected a native {expected_platform} image for {tag}, found {platform}")
    return {"tag": tag, "id": image_id, "platform": platform}


def build_image(dockerfile, tag, platform):
    checked("docker", "buildx", "build", "--platform", platform, "--load",
            "--tag", tag, "--file", str(dockerfile), str(dockerfile.parent))
    return inspect_image(tag, platform)


def prepare(run, task_name, scratch):
    suite = Path(__file__).resolve().parent
    registry = suite / "registry.json"
    entry = task_entries(registry)[task_name]
    revision = entry["git_commit_id"]
    with TemporaryDirectory(prefix="deep-swe-source.", dir=scratch) as checkout:
        git = ("git", "-C", checkout)
        checked(*git, "init", "--quiet")
        checked(*git, "remote", "add", "origin", entry["git_url"])
        checked(*git, "fetch", "--depth=1", "--filter=blob:none", "origin", revision)
        checked(*git, "sparse-checkout", "set", entry["path"])
        checked(*git, "checkout", "--detach", revision)
        task = run / "task" / task_name
        shutil.copytree(Path(checkout) / entry["path"], task)

    config = task / "task.toml"
    original = config.read_text()
    data = tomllib.loads(original)
    if data["verifier"]["environment_mode"] != "separate":
        raise ValueError("DeepSWE requires a separate verifier")
    upstream_agent = data["environment"]["docker_image"]
    agent_file = task / "environment/Dockerfile"
    verifier_file = task / "tests/Dockerfile"
    platform = os.environ["TALOS_BENCH_PLATFORM"]
    prefix = f"talos-bench-deepswe-{task_name}"
    tag = f"{revision[:7]}-{platform.split('/')[1]}"
    verifier_tag = f"{prefix}-verifier:{tag}"
    base = None
    if platform == "linux/arm64":
        base = ARM64_BASE
        agent_tag = f"{prefix}:{tag}"
        agent_file.write_text(replace_once(
            agent_file.read_text(), f"^FROM {re.escape(MARS_BASE)}:latest$", f"FROM {base}",
        ))
        verifier_file.write_text(replace_once(
            verifier_file.read_text(), f"^FROM {re.escape(upstream_agent)}$", f"FROM {agent_tag}",
        ))
        agent = build_image(agent_file, agent_tag, platform)
        original = replace_once(original, f'^docker_image = "{re.escape(upstream_agent)}"$',
                                f'docker_image = "{agent_tag}"')
    else:
        checked("docker", "pull", "--platform", platform, upstream_agent)
        agent = inspect_image(upstream_agent, platform)
    config.write_text(replace_once(
        original,
        r"^\[verifier\.environment\]$",
        f'[verifier.environment]\ndocker_image = "{verifier_tag}"',
    ))

    verifier = build_image(verifier_file, verifier_tag, platform)
    provenance = run / "provenance"
    provenance.mkdir(exist_ok=True)
    write_json(provenance / f"{task_name}.json", {
        "suite": "deep-swe",
        "task": task_name,
        "sourceRevision": revision,
        "taskBaseCommit": data["metadata"]["base_commit_hash"],
        "upstreamImage": upstream_agent,
        "arm64Base": base,
        "agentImage": agent,
        "verifierImage": verifier,
        "verifierFiles": {
            path.name: hashlib.sha256(path.read_bytes()).hexdigest()
            for path in sorted((task / "tests").iterdir())
            if path.is_file() and path != verifier_file
        },
    })


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("task_name", metavar="task")
    parser.add_argument("scratch", type=Path)
    prepare(**vars(parser.parse_args()))
