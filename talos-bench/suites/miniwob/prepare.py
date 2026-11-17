"""Materialize local Harbor tasks from the pinned MiniWoB++ release."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys

from summarize import read_dataset, write_json


SUITE = Path(__file__).resolve().parent


def prepare(run, cache, names):
    image = os.environ.get("TALOS_BENCH_BROWSER_IMAGE")
    if not image:
        raise ValueError("Set TALOS_BENCH_BROWSER_IMAGE to a local packaged Talos browser image")
    info, = json.loads(subprocess.check_output(["docker", "image", "inspect", image]))
    platform = f"{info['Os']}/{info['Architecture']}"
    if platform != os.environ["TALOS_BENCH_PLATFORM"]:
        raise ValueError(f"The packaged browser image uses {platform}, expected the native platform")

    registry = read_dataset(SUITE / "registry.json")
    upstream = registry["upstream"]
    checkout = cache / f"miniwob-upstream-{upstream['tag']}"
    if not checkout.is_dir():
        subprocess.run(["git", "clone", "--depth", "1", "--branch", upstream["tag"],
                        upstream["repository"], checkout], check=True)
    actual = subprocess.check_output(["git", "-C", checkout, "rev-parse", "HEAD"], text=True).strip()
    if actual != upstream["revision"]:
        raise ValueError(f"The MiniWoB++ checkout does not match the registered revision: {actual}")
    subprocess.run(["git", "-C", checkout, "diff", "--quiet", "HEAD", "--", "miniwob/html"], check=True)

    entries = {entry["name"]: entry for entry in registry["tasks"]}
    provenance = run / "provenance"
    provenance.mkdir()
    for name in names:
        entry = entries[name]
        case = run / "task" / name
        environment = case / "environment"
        tests = case / "tests"
        environment.mkdir(parents=True)
        tests.mkdir()
        shutil.copytree(checkout / "miniwob/html", environment / "html")
        shutil.copyfile(SUITE / "environment.Dockerfile", environment / "Dockerfile")
        shutil.copyfile(SUITE / "docker-compose.yaml", environment / "docker-compose.yaml")
        shutil.copyfile(
            SUITE.parents[2] / "deploy/docker/talos-worker/chromium-seccomp.json",
            environment / "chromium-seccomp.json",
        )
        shutil.copyfile(SUITE / "test.sh", tests / "test.sh")
        url = f"http://127.0.0.1:8765/miniwob/{name}.html?mode=test&seed={entry['seed']}"
        write_json(environment / "episode-config.json", {"task": name, "seed": entry["seed"], "url": url})
        (case / "task.toml").write_text(
            'schema_version = "1.1"\n'
            'artifacts = [{ source = "/logs/artifacts/episode.json", destination = "episode.json" }]\n'
            f'[task]\nname = "miniwob/{name}"\nversion = "{registry["version"]}"\n'
            '[agent]\ntimeout_sec = 180.0\n'
            '[verifier]\ntimeout_sec = 15.0\n'
            '[environment]\nbuild_timeout_sec = 360.0\ncpus = 2\nmemory_mb = 4096\nworkdir = "/home/talos"\n'
        )
        (case / "instruction.md").write_text(
            f"Open the MiniWoB++ browser task at {url}. "
            "Follow the instruction displayed at the top of the page. It starts its own timer when loaded, "
            "so navigate, read the page and act promptly. Use the built-in Talos Browser MCP for all "
            "interaction. Do not use shell commands, direct HTTP, source files, reloads or other tools "
            "to perform the task. Report whether you completed it.\n"
        )
        write_json(provenance / f"{name}.json", {
            "task": name, "datasetRevision": actual, "seed": entry["seed"], "dataMode": "test",
            "upstreamFiles": {
                path: hashlib.sha256((environment / "html" / path).read_bytes()).hexdigest()
                for path in (f"miniwob/{name}.html", "core/core.js")
            },
            "browserImage": image, "browserImageId": info["Id"], "platform": platform,
            "browserImageRevision": info["Config"].get("Labels", {}).get("org.opencontainers.image.revision"),
        })


if __name__ == "__main__":
    prepare(Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3:])
