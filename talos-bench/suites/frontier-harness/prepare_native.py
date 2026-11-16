"""Materialize the pinned music-harmony verifier for native ARM64 Docker."""

import asyncio
import json
from pathlib import Path
import re
import shutil
import sys
import tomllib

from harbor.models.task.id import PackageTaskId
from harbor.publisher.packager import Packager
from harbor.tasks.client import TaskClient

from summarize import read_dataset, split_tasks, write_json


MUSIC_TASK = "terminal-bench/music-harmony"


def copy_with_prepared_images(source, task, images):
    shutil.copytree(source, task)
    config_path = task / "task.toml"
    content = config_path.read_text()
    expected = tomllib.loads(content)
    for role, image in images.items():
        section = "environment" if role == "agent" else "verifier.environment"
        environment = expected["environment"] if role == "agent" else expected["verifier"]["environment"]
        environment["docker_image"] = image
        table = re.search(rf"(?m)^[ \t]*\[{re.escape(section)}\][^\n]*(?:\n|$)", content)
        if not table:
            raise ValueError(f"Missing task config table [{section}]")
        start = table.end()
        next_table = re.search(r"(?m)^[ \t]*\[\[?[^\]\n]+\]\]?[ \t]*(?:#.*)?$", content[start:])
        end = start + next_table.start() if next_table else len(content)
        # Edit the image field in place so mixed arrays and other task metadata survive.
        body, count = re.subn(
            r'(?m)^([ \t]*docker_image[ \t]*=[ \t]*)("[^"\n]*"|\'[^\'\n]*\')',
            lambda match: match[1] + json.dumps(image), content[start:end],
        )
        if not count:
            separator = "" if content[:start].endswith("\n") else "\n"
            body = f"{separator}docker_image = {json.dumps(image)}\n{body}"
        content = content[:start] + body + content[end:]
    if tomllib.loads(content) != expected:
        raise ValueError("Could not preserve the upstream task configuration")
    config_path.write_text(content)
    digest, _ = Packager.compute_content_hash(task)
    return f"sha256:{digest}"


def copy_for_native_verifier(source, task, verifier_image):
    shutil.copytree(source, task)
    config = task / "task.toml"
    original = config.read_text()
    expected = tomllib.loads(original)
    expected["verifier"]["environment"].pop("docker_image")
    adapted = original.replace(f'docker_image = "{verifier_image}"\n', "", 1)
    if tomllib.loads(adapted) != expected:
        raise ValueError("The native task must change only the verifier image")
    config.write_text(adapted)
    digest, _ = Packager.compute_content_hash(task)
    return f"sha256:{digest}"


def prepare(registry, run_dir, requested):
    dataset = read_dataset(registry)
    entry = next(task for task in dataset["tasks"] if task["name"] == MUSIC_TASK)
    org, name = MUSIC_TASK.split("/")
    task_id = PackageTaskId(org=org, name=name, ref=entry["digest"])
    source = task_id.get_local_path()
    if not source.is_dir():
        downloaded = asyncio.run(TaskClient().download_tasks([task_id]))
        source, = downloaded.paths
    digest, _ = Packager.compute_content_hash(source)
    if f"sha256:{digest}" != entry["digest"]:
        raise ValueError(f"The downloaded task differs from the {MUSIC_TASK} pin")

    task = run_dir / "native-task" / name
    verifier_image = tomllib.loads((source / "task.toml").read_text())["verifier"]["environment"]["docker_image"]
    adapted_digest = copy_for_native_verifier(source, task, verifier_image)
    provenance = run_dir / "provenance"
    provenance.mkdir()
    write_json(provenance / f"{name}.json", {
        "upstreamDatasetRef": f"{dataset['source']}@{dataset['digest']}",
        "upstreamTaskRef": f"{MUSIC_TASK}@{entry['digest']}",
        "localTaskDigest": adapted_digest,
        "upstreamVerifierImage": verifier_image,
        "adaptation": "Build the unmodified upstream tests/Dockerfile on linux/arm64",
    })

    selected = split_tasks(requested)
    remaining = [task_name for task_name in selected if task_name != MUSIC_TASK]
    job = {"tasks": [{"path": str(task)}]}
    if remaining:
        job["datasets"] = [{"name": dataset["source"], "ref": dataset["digest"], "task_names": remaining}]
        job["artifacts"] = [path for task in dataset["tasks"] if task["name"] in remaining
                            for path in task["submission_paths"]]
    write_json(run_dir / "native-job.json", job)
    entry["name"] = name
    write_json(run_dir / "native-registry.json", [dataset])
    print(",".join(name if task_name == MUSIC_TASK else task_name for task_name in selected))


if __name__ == "__main__":
    prepare(Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3])
