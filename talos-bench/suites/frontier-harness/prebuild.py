"""Prepare an admitted task's original Dockerfile outside the scored trial."""

import argparse
import json
from pathlib import Path
import subprocess
import time

from harbor.publisher.packager import Packager


PLATFORM = "linux/arm64"
GSEA_ARM64_TASK = "sha256:7c0512e617cd4c0a0b595ebfb81258495d7e4a12c003cd0592c1137aa986a85d"


def native_image(image):
    result, = json.loads(subprocess.check_output(["docker", "image", "inspect", image], text=True))
    if f"{result['Os']}/{result['Architecture']}" != PLATFORM:
        raise ValueError(f"Image {image} does not use {PLATFORM}")
    return result["Id"]


def task_source(task):
    source = Path(task["taskPath"])
    digest, _ = Packager.compute_content_hash(source)
    if f"sha256:{digest}" != task["digest"]:
        raise ValueError("Cached task does not match the published digest")
    return source


def prepare_image(task, role, prepared):
    source = task_source(task)
    name = task["name"].rsplit("/", 1)[-1]
    digest = task["digest"].removeprefix("sha256:")
    dockerfile = source / task[role]["dockerfile"]
    adaptation = Path(__file__).with_name("images") / "gsea-proteomics-arm64.Dockerfile"
    use_adaptation = task["digest"] == GSEA_ARM64_TASK and role == "agent"
    image = f"talos-bench-local/{name}-{role}:{digest[:12]}"
    output = prepared / name
    output.mkdir(parents=True, exist_ok=True)
    if use_adaptation:
        combined = output / f"{role}.Dockerfile"
        combined.write_bytes(dockerfile.read_bytes().rstrip() + b"\n\n" + adaptation.read_bytes())
        context, dockerfile = dockerfile.parent, combined
    else:
        context = dockerfile.parent
    command = ["docker", "buildx", "build", "--platform", PLATFORM, "--progress", "plain", "--load",
               "--tag", image, "--file", str(dockerfile), str(context)]
    start = time.monotonic()
    with (output / f"{role}-build.log").open("w") as log:
        subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, check=True)
        image_id = native_image(image)
        architecture = subprocess.check_output([
            "docker", "run", "--rm", "--platform", PLATFORM, "--network", "none",
            "--entrypoint", "uname", image, "-m",
        ], stderr=log, text=True, timeout=60).strip()
        if architecture != "aarch64":
            raise ValueError(f"Image {image} reported runtime architecture {architecture}")
        print(f"Image {image_id} started as {architecture}", file=log)
    receipt = {"taskRef": f"{task['name']}@{task['digest']}", "image": image, "imageId": image_id,
               "platform": PLATFORM, "command": command, "elapsedSeconds": time.monotonic() - start}
    if use_adaptation:
        receipt["adaptation"] = f"{adaptation.parent.name}/{adaptation.name} appended to the verified upstream recipe"
    (output / f"{role}.json").write_text(json.dumps(receipt, indent=2) + "\n")
    print(f"Prepared {image} ({image_id})", flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("preflight", type=Path)
    parser.add_argument("prepared", type=Path)
    parser.add_argument("task", help="Qualified admitted task name")
    parser.add_argument("role", choices=("agent", "verifier"))
    options = parser.parse_args()
    inventory = json.loads(options.preflight.read_text())
    platform = subprocess.check_output(["docker", "version", "--format", "{{.Server.Os}}/{{.Server.Arch}}"],
                                       text=True).strip()
    if platform != PLATFORM or inventory["platform"] != PLATFORM:
        parser.error(f"The inventory and Docker daemon must both use {PLATFORM}")
    task = next((task for task in inventory["tasks"] if task["name"] == options.task
                 and task["status"] in {"ready", "needsNativeBuild"}), None)
    if task is None or not task[options.role].get("dockerfile"):
        parser.error("The selected role must have an admitted native Dockerfile")
    prepare_image(task, options.role, options.prepared.resolve())


if __name__ == "__main__":
    main()
