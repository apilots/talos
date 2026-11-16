# /// script
# requires-python = ">=3.12"
# dependencies = ["harbor==0.25.0", "PyYAML==6.0.3"]
# ///
"""Inspect the pinned FrontierHarness tasks without building or starting containers."""

import argparse
import asyncio
import json
from pathlib import Path
import re
import subprocess
import tomllib

import yaml
from harbor.models.task.id import PackageTaskId
from harbor.publisher.packager import Packager
from harbor.registry.client.package import PackageDatasetClient
from harbor.tasks.client import TaskClient


PLATFORM = "linux/arm64"
STATUS_ORDER = {"ready": 0, "needsNativeBuild": 1, "unresolved": 2, "blocked": 3}
METADATA_ONLY = {
    "bun-sourcemap-leak": "vulnerability reproduction or data-exposure task",
    "dna-assembly": "genetic assembly task",
    "dna-insert": "genetic engineering task",
    "feal-linear-cryptanalysis": "cryptanalytic attack task",
    "vulnerable-secret": "vulnerability exploitation or secret-extraction task",
}
GUEST_EMULATION = {
    "install-windows-3.11": "requires booting a Windows guest in QEMU",
    "make-doom-for-mips": "requires running a MIPS ELF in the supplied JavaScript VM",
}
PRIORITY = {
    "log-summary-date-ranges", "music-harmony", "regex-log", "polyglot-c-py",
    "large-scale-text-editing", "schemelike-metacircular-eval", "adaptive-rejection-sampler",
    "chess-best-move",
}
FROM = re.compile(r"^[ \t]*FROM(?:[ \t]+--platform=(\S+))?[ \t]+(\S+)(?:[ \t]+AS[ \t]+(\S+))?[ \t]*$", re.I | re.M)
FOREIGN_ARCH = re.compile(r"(?:linux[/_-](?:amd64|386|ppc64le|s390x|riscv64)|x86_64|x86-64|amd64|x64)", re.I)
EMULATOR = re.compile(r"\b(?:qemu-system-\w+|qemu-user(?:-static)?|dosbox|box64|rosetta)\b", re.I)


def native(platform):
    return platform == PLATFORM or platform.startswith(f"{PLATFORM}/")


def image_platforms(ref):
    command = ["docker", "buildx", "imagetools", "inspect"]

    def inspect(*args):
        result = subprocess.run([*command, *args, ref], capture_output=True, check=True, text=True, timeout=20)
        return json.loads(result.stdout)

    try:
        raw = inspect("--raw")
        if "manifests" in raw:
            platforms = [item.get("platform", {}) for item in raw["manifests"]]
        else:
            platforms = [inspect("--format", "{{json .Image}}")]
        result = sorted({
            "/".join(str(config[key]) for key in ("os", "architecture", "variant") if config.get(key))
            for config in platforms if config.get("os") not in {None, "unknown"}
            and config.get("architecture") not in {None, "unknown"}
        })
        return result, None if result else "registry did not identify any image platforms"
    except (subprocess.SubprocessError, ValueError, AttributeError) as error:
        return [], f"image manifest inspection failed ({type(error).__name__})"


class Images:
    def __init__(self, concurrency):
        self.pending = {}
        self.slots = asyncio.Semaphore(concurrency)

    async def inspect(self, ref):
        if ref not in self.pending:
            self.pending[ref] = asyncio.create_task(self.fetch(ref))
        return await self.pending[ref]

    async def fetch(self, ref):
        async with self.slots:
            return await asyncio.to_thread(image_platforms, ref)


def docker_host():
    platform = subprocess.check_output(
        ["docker", "version", "--format", "{{.Server.Os}}/{{.Server.Arch}}"], text=True,
    ).strip()
    if platform != PLATFORM:
        raise ValueError(f"Expected a native {PLATFORM} Docker daemon, found {platform}")
    cpus, memory = subprocess.check_output(
        ["docker", "info", "--format", "{{.NCPU}} {{.MemTotal}}"], text=True,
    ).split()
    # The local Apple Silicon Docker daemon exposes no supported GPU devices.
    return {"cpus": int(cpus), "memoryMb": int(memory) // (1024 * 1024), "gpus": 0}


def recipe_constraints(text):
    reasons = []
    for platform, _, _ in FROM.findall(text):
        if platform and platform not in {"$BUILDPLATFORM", "${BUILDPLATFORM}", "$TARGETPLATFORM", "${TARGETPLATFORM}"}:
            if not native(platform):
                reasons.append(f"Dockerfile FROM platform {platform} needs native review")
    if FOREIGN_ARCH.search(text):
        reasons.append("Dockerfile contains a foreign architecture reference")
    if EMULATOR.search(text):
        reasons.append("Dockerfile references an emulator and requires manual review")
    return reasons


async def inspect_recipe_bases(text, images):
    reasons = []
    bases = {}
    stages = set()
    for _, base, alias in FROM.findall(text):
        if base.lower() not in stages | {"scratch"}:
            if "$" in base:
                reasons.append(f"Dockerfile base {base} is dynamic")
            else:
                platforms, error = await images.inspect(base)
                bases[base] = platforms
                if error or not any(map(native, platforms)):
                    reasons.append(f"Dockerfile base {base}: {error or 'no published ARM64 variant'}")
        if alias:
            stages.add(alias.lower())
    if not FROM.search(text):
        reasons.append("Dockerfile has no statically identifiable base image")
    return bases, reasons


async def inspect_compose(path, images):
    services = yaml.safe_load(path.read_text()).get("services", {})
    details = {}
    for name, service in services.items():
        image = service.get("image")
        if image and "$" not in image:
            platforms, error = await images.inspect(image)
        else:
            platforms, error = [], "no statically identified image"
        details[name] = {"image": image, "imagePlatforms": platforms,
                         "platform": service.get("platform"), "hasBuild": "build" in service}
        if error:
            details[name]["imagePlatformError"] = error
    return details


async def inspect_stage(config, folder, task_path, host, images):
    image = config.get("docker_image")
    recipe = folder / "Dockerfile"
    platforms, error = await images.inspect(image) if image else ([], None)
    stage = {
        "os": config.get("os", "linux"), "image": image, "imagePlatforms": platforms,
        "dockerfile": str(recipe.relative_to(task_path)) if recipe.is_file() else None,
        "cpu": config.get("cpus"), "memoryMb": config.get("memory_mb"),
        "storageMb": config.get("storage_mb"), "gpus": config.get("gpus", 0),
    }
    if error:
        stage["imagePlatformError"] = error
    reasons = []
    if stage["os"] != "linux":
        reasons.append(f"requires {stage['os']} containers")
    for field, limit in (("cpu", "cpus"), ("memoryMb", "memoryMb"), ("gpus", "gpus")):
        if stage[field] is not None and stage[field] > host[limit]:
            reasons.append(f"requires {stage[field]} {field}, host provides {host[limit]}")
    if reasons:
        return stage, "blocked", reasons
    for filename in ("docker-compose.yaml", "docker-compose.yml"):
        compose = folder / filename
        if compose.is_file():
            services = await inspect_compose(compose, images)
            stage["composeServices"] = services
            foreign_services = [name for name, detail in services.items()
                                if detail["imagePlatforms"] and not any(map(native, detail["imagePlatforms"]))]
            reason = (f"Compose services lack a native published image: {', '.join(foreign_services)}"
                      if foreign_services else "Compose services require platform review")
            return stage, "unresolved", [reason]
    if recipe.is_file():
        text = "\n".join(line for line in recipe.read_text().splitlines() if not line.lstrip().startswith("#"))
        reasons = recipe_constraints(text)
    if image and any(map(native, platforms)):
        return stage, "unresolved" if reasons else "ready", reasons
    if not recipe.is_file():
        return stage, "unresolved" if error else "blocked", [error or "no native image or build recipe"]
    stage["baseImages"], base_reasons = await inspect_recipe_bases(text, images)
    reasons.extend(base_reasons)
    if reasons:
        return stage, "unresolved", reasons
    if error:
        return stage, "needsNativeBuild", [f"{error}; use the inspected upstream Dockerfile with explicit native build"]
    return stage, "needsNativeBuild", ["no published ARM64 image; upstream Dockerfile is a native build candidate"]


def inspection_error(error):
    if isinstance(error, ExceptionGroup):
        return "; ".join(inspection_error(child) for child in error.exceptions)
    response = getattr(error, "response", None)
    message = f"HTTP {response.status_code}" if response is not None else str(error)
    return f"{type(error).__name__}: {message}" if message else type(error).__name__


async def inspect_task(task_id, host, images, slots, download_timeout):
    row = {"name": task_id.get_name(), "digest": task_id.ref, "taskPath": None, "agent": None, "verifier": None}
    name = task_id.name
    if name in METADATA_ONLY:
        return {**row, "status": "blocked", "reasons": [METADATA_ONLY[name], "metadata only"]}
    if name in GUEST_EMULATION:
        return {**row, "status": "blocked", "reasons": [GUEST_EMULATION[name]]}
    try:
        async with slots:
            async with asyncio.timeout(download_timeout):
                task_path, = (await TaskClient().download_tasks([task_id])).paths
            digest, _ = await asyncio.to_thread(Packager.compute_content_hash, task_path)
            if f"sha256:{digest}" != task_id.ref:
                raise ValueError("pinned task content hash mismatch")
        row["taskPath"] = str(task_path.resolve())
        config = tomllib.loads((task_path / "task.toml").read_text())
        verifier = config.get("verifier", {})
        mode = verifier.get("environment_mode", "shared")
        stages = [inspect_stage(config.get("environment", {}), task_path / "environment", task_path, host, images)]
        if mode == "separate":
            stages.append(inspect_stage(verifier.get("environment", {}), task_path / "tests", task_path, host, images))
        assessments = await asyncio.gather(*stages)
        statuses, reasons = [], []
        for key, (detail, status, issues) in zip(("agent", "verifier"), assessments):
            row[key] = detail
            statuses.append(status)
            reasons.extend(f"{key}: {issue}" for issue in issues)
        row["verifier"] = {**(row["verifier"] or {}), "mode": mode}
        if config.get("steps"):
            statuses.append("unresolved")
            reasons.append("multi-step verifier environments require review")
        return {**row, "status": max(statuses, key=STATUS_ORDER.get), "reasons": reasons}
    except (OSError, ValueError, TimeoutError, ExceptionGroup, yaml.YAMLError) as error:
        return {**row, "status": "unresolved",
                "reasons": [f"task metadata inspection failed ({inspection_error(error)})"]}


def save(path, report):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(report, indent=2) + "\n")
    temporary.replace(path)


async def preflight(output, retry_unresolved):
    registry, = json.loads(Path(__file__).with_name("registry.json").read_text())
    ref = f"{registry['source']}@{registry['digest']}"
    dataset = await PackageDatasetClient().get_dataset_metadata(ref)
    if dataset.version != registry["digest"]:
        raise ValueError("Harbor resolved a different FrontierHarness dataset version")
    task_ids = list(dataset.task_ids)
    if len(task_ids) != 62 or any(not isinstance(task, PackageTaskId) or not (task.ref or "").startswith("sha256:") for task in task_ids):
        raise ValueError("Expected 62 digest-pinned FrontierHarness tasks")
    host = docker_host()
    report = {"datasetRef": ref, "platform": PLATFORM, "host": host, "complete": False,
              "tasks": [{"name": task.get_name(), "digest": task.ref, "status": "unresolved",
                         "reasons": ["preflight in progress"]} for task in task_ids]}
    if retry_unresolved:
        previous = json.loads(output.read_text())
        pins = [(row["name"], row["digest"]) for row in report["tasks"]]
        if not previous["complete"] or previous["datasetRef"] != ref or previous["platform"] != PLATFORM or (
            previous["host"] != host or [(row["name"], row["digest"]) for row in previous["tasks"]] != pins
        ):
            raise ValueError("Retry requires a completed report for the same dataset and Docker host")
        report = {**previous, "complete": False}
    output.parent.mkdir(parents=True, exist_ok=True)
    save(output, report)
    concurrency = 2 if retry_unresolved else 6
    images, slots = Images(concurrency), asyncio.Semaphore(concurrency)
    download_timeout = 150 if retry_unresolved else 90

    async def check(index, task_id):
        return index, await inspect_task(task_id, host, images, slots, download_timeout)

    order = sorted(((index, task_id) for index, task_id in enumerate(task_ids)
                    if report["tasks"][index]["status"] == "unresolved"),
                   key=lambda item: item[1].name not in PRIORITY)
    checks = [asyncio.create_task(check(index, task)) for index, task in order]
    for checked in asyncio.as_completed(checks):
        index, row = await checked
        if retry_unresolved:
            prior = report["tasks"][index]
            row["previousReasons"] = [*prior.get("previousReasons", []), *prior["reasons"]]
        report["tasks"][index] = row
        save(output, report)
        print(f"{row['status']:<17} {row['name']}", flush=True)
    report["complete"] = True
    save(output, report)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--retry-unresolved", action="store_true", help="recheck unresolved rows in a completed report")
    args = parser.parse_args()
    asyncio.run(preflight(args.output, args.retry_unresolved))
