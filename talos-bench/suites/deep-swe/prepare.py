"""Build a native ARM64 agent image and separate verifier for a pinned DeepSWE case."""

import argparse
import hashlib
from pathlib import Path
import re
import shutil
import subprocess
from tempfile import TemporaryDirectory
import tomllib

from summarize import task_entries, write_json


def checked(*command):
    subprocess.run(command, check=True)


def replace_once(text, pattern, value):
    rewritten, matches = re.subn(pattern, lambda match: value, text, flags=re.MULTILINE)
    if matches != 1:
        raise ValueError(f"Expected one match for {pattern}; found {matches}")
    return rewritten


def build_image(dockerfile, tag):
    checked("docker", "buildx", "build", "--platform", "linux/arm64", "--load",
            "--tag", tag, "--file", str(dockerfile), str(dockerfile.parent))
    output = subprocess.check_output(
        ["docker", "image", "inspect", "--format", "{{.Os}}/{{.Architecture}} {{.Id}}", tag],
        text=True,
    ).strip()
    platform, image_id = output.split()
    if platform != "linux/arm64":
        raise ValueError(f"Expected a native linux/arm64 image for {tag}, found {platform}")
    return {"tag": tag, "id": image_id, "platform": platform}


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
    original_agent = agent_file.read_text()
    original_verifier = verifier_file.read_text()
    recipes = suite / "cases" / task_name
    agent_recipe = (recipes / "agent.Dockerfile").read_text()
    verifier_recipe = (recipes / "verifier.Dockerfile").read_text()
    base, = re.findall(r"^FROM (\S+@sha256:[a-f0-9]{64})$", agent_recipe, re.MULTILINE)
    if agent_recipe != replace_once(original_agent, r"^FROM \S+$", f"FROM {base}"):
        raise ValueError("The local agent Dockerfile differs from the upstream recipe beyond its base")
    agent_arg = "ARG TALOS_BENCH_AGENT_IMAGE"
    expected_verifier = agent_arg + "\n" + replace_once(
        original_verifier, f"^FROM {re.escape(upstream_agent)}$",
        "FROM ${TALOS_BENCH_AGENT_IMAGE}",
    )
    if verifier_recipe != expected_verifier:
        raise ValueError("The local verifier Dockerfile differs from the upstream recipe beyond its base")
    prefix = f"talos-bench-deepswe-{task_name}"
    tag = f"{revision[:7]}-arm64"
    agent_tag = f"{prefix}:{tag}"
    verifier_tag = f"{prefix}-verifier:{tag}"
    agent_file.write_text(agent_recipe)
    verifier_file.write_text(replace_once(verifier_recipe, f"^{agent_arg}$", f"{agent_arg}={agent_tag}"))
    config.write_text(replace_once(
        replace_once(original, f'^docker_image = "{re.escape(upstream_agent)}"$',
                     f'docker_image = "{agent_tag}"'),
        r"^\[verifier\.environment\]$",
        f'[verifier.environment]\ndocker_image = "{verifier_tag}"',
    ))

    agent = build_image(agent_file, agent_tag)
    verifier = build_image(verifier_file, verifier_tag)
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
