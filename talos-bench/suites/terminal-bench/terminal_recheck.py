"""Run the pinned Harbor verifier again using a trial's archived submission."""

import argparse
import asyncio
import os
from pathlib import Path
import shlex
import subprocess
from tempfile import mkdtemp

from harbor.agents.nop import NopAgent
from harbor.models.task.id import GitTaskId
from harbor.publisher.packager import Packager
from harbor.tasks.client import TaskClient

from summarize import read_json, task_entries, write_json

SUITE = Path(__file__).resolve().parent
BENCH = SUITE.parents[1]
REPOSITORY = BENCH.parent
SOURCE_FIELDS = ("git_url", "git_commit_id", "path")


def pinned_task(lock):
    name = (lock.get("name") or "").rsplit("/", 1)[-1]
    registries = [SUITE / "registry.json", *sorted(SUITE.glob("registry-*.json"))]
    for registry in registries:
        task = task_entries(registry).get(name)
        if task and all(lock.get(key) == task[key] for key in SOURCE_FIELDS):
            return task
    raise SystemExit("The trial task does not match a pinned Talos benchmark case")


def submission_files(trial, task):
    root = (trial / "artifacts").resolve()
    manifest = read_json(root / "manifest.json")
    records = {entry["source"]: entry for entry in manifest}
    for source in task["submission_paths"]:
        record = records.get(source, {})
        path = trial / record.get("destination", "")
        if (record.get("status") != "ok" or not path.is_relative_to(root) or path.resolve() != path
                or not (path.is_file() or path.is_dir())):
            raise ValueError(f"Missing regular archived submission for {source}")
        if path.is_dir() and any(child.is_symlink() or not (child.is_file() or child.is_dir())
                                 for child in path.rglob("*")):
            raise ValueError(f"Archived directory {source} contains a link or special file")
        yield source, path


class SubmissionAgent(NopAgent):
    def __init__(self, *, source_trial, **kwargs):
        super().__init__(**kwargs)
        trial = Path(source_trial)
        task = pinned_task(read_json(trial / "lock.json").get("task", {}))
        self.submission = list(submission_files(trial, task))

    @staticmethod
    def name():
        return "talos-bench-recheck"

    async def run(self, instruction, environment, context):
        for source, path in self.submission:
            target = source if path.is_dir() else str(Path(source).parent)
            result = await environment.exec(f"mkdir -p -- {shlex.quote(target)}")
            if result.return_code:
                raise RuntimeError(f"Could not prepare the verifier input directory {target}")
            if path.is_dir():
                await environment.upload_dir(path, source)
            else:
                await environment.upload_file(path, source)


def recheck(trial):
    lock = read_json(trial / "lock.json").get("task", {})
    task = pinned_task(lock)
    try:
        list(submission_files(trial, task))
    except ValueError as error:
        raise SystemExit(str(error)) from None

    task_id = GitTaskId(**{key: task[key] for key in SOURCE_FIELDS})
    task_dir = task_id.get_local_path()
    if not task_dir.is_dir():
        downloaded = asyncio.run(TaskClient().download_tasks([task_id]))
        task_dir, = downloaded.paths
    digest, _ = Packager.compute_content_hash(task_dir)
    if lock.get("digest") != f"sha256:{digest}":
        raise SystemExit("The pinned task contents differ from the source trial lock")

    environment = dict(os.environ, DOCKER_DEFAULT_PLATFORM="linux/arm64",
                       HARBOR_TELEMETRY="0", PYTHONPATH=os.pathsep.join(map(str, (SUITE, BENCH))))
    platform = subprocess.check_output(
        ["docker", "version", "--format", "{{.Server.Os}}/{{.Server.Arch}}"],
        env=environment, text=True,
    ).strip()
    if platform != "linux/arm64":
        raise SystemExit("A native linux/arm64 Docker daemon is required")

    output = REPOSITORY / "output/talos-bench"
    output.mkdir(parents=True, exist_ok=True)
    run = Path(mkdtemp(prefix="recheck.", dir=output))
    write_json(run / "source.json", {"sourceTrial": str(trial), "task": task["name"],
                                    "taskDigest": lock["digest"]})
    subprocess.run([
        "harbor", "run", "--path", str(task_dir), "--env", "docker", "--force-build",
        "--extra-docker-compose", str(SUITE / "arm64.compose.yaml"),
        "--agent", "terminal_recheck:SubmissionAgent", "--agent-kwarg", f"source_trial={trial}",
        "--job-name", "recheck",
        "--jobs-dir", str(run), "--yes",
    ], env=environment, check=True)
    result_path, = (run / "recheck").glob("*__*/result.json")
    result = read_json(result_path)
    reward = ((result.get("verifier_result") or {}).get("rewards") or {}).get("reward")
    if result.get("exception_info") or reward is None:
        raise SystemExit(f"The verifier did not return a score. See {result_path}")
    print(f"Recheck reward: {reward}. See {result_path}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("trial", type=Path)
    recheck(parser.parse_args().trial.resolve())
