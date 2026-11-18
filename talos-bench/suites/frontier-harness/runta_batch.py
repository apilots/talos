"""Run one isolated Runta invocation per registered FrontierHarness task.

A started case is never launched again by resume. Inspect its log and retained
runtime when the controller or evidence transfer fails.
"""

import argparse
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
REGISTRY = Path(__file__).with_name("registry.json")


def write_json(path, value):
    temp = path.with_suffix(".tmp")
    temp.write_text(json.dumps(value, indent=2) + "\n")
    temp.replace(path)


def now():
    return datetime.now(timezone.utc).isoformat()


def config():
    names = ("TALOS_BENCH_RUNTA_CHECKPOINT", "TALOS_BENCH_RUNTA_SECRET", "TALOS_BENCH_MODEL",
             "TALOS_BENCH_REASONING_EFFORT")
    missing = [name for name in names if not os.environ.get(name)]
    if missing:
        raise ValueError("Missing environment: " + ", ".join(missing))
    paths = subprocess.check_output(["git", "-C", str(ROOT), "ls-files", "-z", "--", "talos-bench"]).split(b"\0")
    digest = hashlib.sha256()
    for raw in filter(None, paths):
        path = os.fsdecode(raw)
        digest.update(raw + b"\0")
        digest.update((ROOT / path).read_bytes())
    return {"checkpoint": os.environ[names[0]], "secret_name": os.environ[names[1]],
            "model": os.environ[names[2]], "effort": os.environ[names[3]],
            "launcher_revision": subprocess.check_output(["git", "-C", str(ROOT), "rev-parse", "HEAD"], text=True).strip(),
            "bench_sha256": digest.hexdigest(), "batch_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
            "registry_sha256": hashlib.sha256(REGISTRY.read_bytes()).hexdigest(),
            "attempts_per_task": 1}


def case_dir(batch, task):
    return batch / "cases" / task.replace("/", "--")


def run_case(batch, task):
    case = case_dir(batch, task)
    case.mkdir(parents=True)
    state = {"task": task, "status": "started", "started_at": now()}
    write_json(case / "state.json", state)
    env = os.environ.copy()
    env.update(TALOS_BENCH_TASKS=task, TALOS_BENCH_REPEATS="1")
    with (case / "launcher.log").open("w") as log:
        result = subprocess.run([str(ROOT / "talos-bench/run.sh"), "frontier-harness@1.1.0", "--backend", "runta"],
                                cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=False)
    lines = (case / "launcher.log").read_text().splitlines()
    output = next((line.removeprefix("Talos benchmark run: ") for line in lines
                   if line.startswith("Talos benchmark run: ")), None)
    runtime = next((line.removeprefix("Runta runtime: ") for line in lines
                    if line.startswith("Runta runtime: ")), None)
    state.update(status="finished", finished_at=now(), exit_code=result.returncode,
                 output=output, runtime=runtime)
    if output:
        result_dir = Path(output)
        receipt = result_dir / "original-job.sha256"
        if receipt.is_file():
            state["archive_sha256"] = receipt.read_text().strip()
            summary = result_dir / "summary.json"
            if summary.is_file():
                value = json.loads(summary.read_text())
                state.update(complete=value.get("complete"), scored_trials=value.get("scoredTrials"),
                             score=value.get("meanScore"))
    write_json(case / "state.json", state)
    return task


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--jobs", type=int, default=1)
    parser.add_argument("--task", action="append", default=[])
    parser.add_argument("--limit", type=int, help="Start at most this many unstarted cases")
    parser.add_argument("--first", help="Start this selected pending case first")
    args = parser.parse_args()
    if args.jobs < 1 or (args.limit is not None and args.limit < 1):
        parser.error("--jobs and --limit must be positive")
    try:
        settings = config()
        dataset, = json.loads(REGISTRY.read_text())
        available = [x["name"] for x in dataset["tasks"]]
        tasks = args.task or available
        if len(tasks) != len(set(tasks)) or set(tasks) - set(available):
            raise ValueError("Tasks must be distinct selectable registry IDs")
        if args.first and args.first not in tasks:
            raise ValueError("--first must name a selected task")
        args.directory.mkdir(parents=True, exist_ok=True)
        with (args.directory / ".lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            manifest = args.directory / "manifest.json"
            requested = {**settings, "tasks": tasks, "dataset": dataset["digest"],
                         "metadata_only": {name: item for name, item in dataset.get("native_exclusions", {}).items()
                                           if item["kind"] == "metadataOnly"}}
            if manifest.exists():
                if json.loads(manifest.read_text()) != requested:
                    raise ValueError("Batch manifest differs; use a new directory")
            else:
                write_json(manifest, requested)
            pending = [task for task in tasks if not case_dir(args.directory, task).exists()]
            if args.first in pending:
                pending.remove(args.first)
                pending.insert(0, args.first)
            if args.limit is not None:
                pending = pending[:args.limit]
            with ThreadPoolExecutor(max_workers=args.jobs) as pool:
                futures = [pool.submit(run_case, args.directory, task) for task in pending]
                for future in as_completed(futures):
                    print(f"{future.result()}: finished", flush=True)
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(str(error))


if __name__ == "__main__":
    main()
