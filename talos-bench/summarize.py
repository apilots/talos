"""Record a benchmark build and summarize the Harbor job beside its raw artifacts."""

import argparse
from datetime import datetime
import hashlib
import json
import os
from pathlib import Path
import subprocess

from runtime.payload import cli_log_lines, run_metrics


def read_json(path):
    return json.loads(path.read_text()) if path.is_file() and path.stat().st_size else {}


def write_json(path, value):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(value, indent=2) + "\n")
    temporary.replace(path)


def elapsed(record):
    started, finished = record.get("started_at"), record.get("finished_at")
    if started and finished:
        return (datetime.fromisoformat(finished) - datetime.fromisoformat(started)).total_seconds()
    return None


def split_tasks(value):
    tasks = [item.strip() for item in value.split(",")]
    if not all(tasks) or len(tasks) != len(set(tasks)):
        raise SystemExit("Select a comma-separated list of distinct task names")
    return tasks


def select_tasks(registry, tasks):
    selected = split_tasks(tasks)
    entries = task_entries(registry)
    unknown = set(selected) - entries.keys()
    if unknown:
        raise SystemExit(f"Unknown Talos benchmark task(s): {', '.join(sorted(unknown))}")
    print("\n".join(selected))


def read_dataset(registry):
    dataset, = read_json(registry)
    return dataset


def task_entries(registry):
    return {entry["name"]: entry for entry in read_dataset(registry)["tasks"]}


def list_suites(suites):
    registries = [*suites.glob("*/registry.json"), *suites.glob("*/registry-*.json")]
    for registry in sorted(registries):
        dataset = read_dataset(registry)
        print(f"{dataset['name']}@{dataset['version']}")
        for task in dataset["tasks"]:
            print(f"  {task['name']}")


def submission_paths(registry, tasks):
    entries = task_entries(registry)
    for source in dict.fromkeys(path for name in split_tasks(tasks)
                                for path in entries[name]["submission_paths"]):
        print(source)


def artifact_identity(artifact):
    if isinstance(artifact, str):
        return "main", artifact
    return artifact.get("service") or "main", artifact["source"]


def record_build(run, repo, binary, source, mode, repeats, tasks):
    def git(*args):
        return subprocess.check_output(["git", "-C", str(repo), *args], text=True).strip()

    with binary.open("rb") as executable:
        sha256 = hashlib.file_digest(executable, "sha256").hexdigest()
    selected = split_tasks(tasks)
    write_json(run / "build.json", {
        "checkoutRevision": git("rev-parse", "HEAD"),
        "checkoutDirty": bool(git("status", "--porcelain")),
        "productSource": source,
        "talosBinarySha256": sha256,
        "platform": os.environ["TALOS_BENCH_PLATFORM"],
        "mode": mode,
        "requestedTasks": selected,
        "requestedTrials": repeats * len(selected),
    })


def observed_seconds(metrics, field):
    samples = [metric[field] for metric in metrics if metric and metric.get(field) is not None]
    return sum(samples) / 1000 if samples else None


def estimate_cost(usage, prices):
    if not usage or not prices or not all(field in prices for field in ("input", "output", "cache_read", "cache_write")):
        return None
    cached = usage.get("cache_read_input_tokens") or 0
    written = usage.get("cache_creation_input_tokens") or 0
    uncached = usage["input_tokens"] - cached - written
    if uncached < 0:
        return None
    return (uncached * prices["input"] + cached * prices["cache_read"]
            + written * prices["cache_write"] + usage["output_tokens"] * prices["output"]) / 1_000_000


def trial_summary(run, directory, entries):
    result = read_json(directory / "result.json")
    task = read_json(directory / "lock.json").get("task", {})
    agent = directory / "agent"
    cli = read_json(agent / "talos-run.json")
    try:
        trace = read_json(agent / "talos-trajectory.json") or {}
        trace_error = None
    except json.JSONDecodeError:
        # Keep the failed trial's other facts even if the CLI was interrupted mid-write.
        trace = {}
        trace_error = "invalidJson"
    calls = [record for entry in trace.get("data") or [] if entry["type"] == "modelRequest"
             for record in entry["request"]["callRecords"]]
    lines = cli_log_lines(agent)
    metrics = run_metrics(lines) or {}
    model_timings = [call.get("metrics") for call in calls]
    tool_timings = []
    compactions = 0
    for line in lines:
        if line.startswith("TOOL_CALL_COMPLETED ") and " metrics=" in line:
            tool_timings.append(json.loads(line.rsplit(" metrics=", 1)[1]))
        elif line.startswith("CONTEXT_COMPACTION_COMPLETED "):
            compactions += 1
    usage = metrics.get("usage")
    if usage is None:
        harbor_usage = result.get("agent_result") or {}
        inputs, outputs = harbor_usage.get("n_input_tokens"), harbor_usage.get("n_output_tokens")
        if inputs is not None and outputs is not None:
            usage = {
                "input_tokens": inputs,
                "output_tokens": outputs,
                "total_tokens": inputs + outputs,
                "cache_read_input_tokens": harbor_usage.get("n_cache_tokens"),
            }
    exception = (result.get("exception_info") or {}).get("exception_type")
    if exception is None and "exit_code" in cli and cli["exit_code"] is None:
        exception = "TalosCliTerminationUnrecorded"
    watch_path = agent / "talos-delegation.jsonl"
    watch_samples = watch_path.read_text().splitlines() if watch_path.is_file() else []
    stop_reason = next((reason for line in reversed(watch_samples)
                        if (reason := json.loads(line).get("stopReason"))), None)
    rewards = (result.get("verifier_result") or {}).get("rewards") or {}
    reward = rewards.get("reward")
    if reward == -1 and exception is None:
        exception = "VerifierInfrastructureFailure"
    task_name = task.get("name", result.get("task_name"))
    if task_name and task_name not in entries:
        task_name = task_name.rsplit("/", 1)[-1]
    entry = entries.get(task_name, {})
    atlas_provenance = read_json(run / "provenance" / f"{task_name}.json") if "revision" in entry else {}
    archive = directory / "artifacts"
    manifest = read_json(archive / "manifest.json")
    captured = {artifact_identity(item): item for item in manifest}
    submissions = []
    for declared in entry.get("submission_paths", []):
        service, source = artifact_identity(declared)
        item = captured.get((service, source), {})
        path = directory / item.get("destination", "")
        exists = path.resolve().is_relative_to(archive.resolve()) and path.exists()
        submission = {
            "source": source,
            "status": item.get("status", "unavailable"),
            "path": str(path.relative_to(run)) if item.get("status") == "ok" and exists else None,
        }
        if service != "main":
            submission["service"] = service
        submissions.append(submission)
    paths = {
        "harborResult": directory / "result.json",
        "trialLog": directory / "trial.log",
        "architecture": agent / "architecture.txt",
        "trajectory": agent / "talos-trajectory.json",
        "transcript": agent / "talos-transcript.json",
        "atif": agent / "trajectory.json",
        "usage": agent / "usage.json",
        "delegationWatch": agent / "talos-delegation.jsonl",
        "delegationEvents": agent / "talos-delegation-events.json",
        "stdout": agent / "talos.stdout.txt",
        "stderr": agent / "talos.stderr.txt",
        "verifier": directory / "verifier",
        "submissionManifest": archive / "manifest.json",
    }
    return {
        "trial": directory.name,
        "task": task_name,
        "taskRevision": task.get("git_commit_id") or entry.get("git_commit_id") or atlas_provenance.get("datasetRevision"),
        "taskDigest": task.get("digest") or result.get("task_checksum"),
        "model": cli.get("model"),
        "reasoningEffort": cli.get("reasoning_effort"),
        "collaborationTools": cli.get("collaboration_tools"),
        "requestIntervalMs": cli.get("request_interval_ms"),
        "score": None if exception or stop_reason else reward,
        "verifierReward": reward,
        "verifierRewards": rewards,
        "trialSeconds": elapsed(result),
        "agentSeconds": elapsed(result.get("agent_execution") or {}),
        "environmentSetupSeconds": elapsed(result.get("environment_setup") or {}),
        "verifierSeconds": elapsed(result.get("verifier") or {}),
        "cliSeconds": cli.get("elapsed_seconds"),
        "cliExitCode": cli.get("exit_code"),
        "exception": exception,
        "stopReason": stop_reason,
        "usage": usage,
        "estimatedCostUsd": estimate_cost(usage, cli.get("prices_per_million")),
        "modelRounds": metrics.get("modelRounds"),
        "modelCallsObserved": len(calls) if trace.get("data") is not None else None,
        "observedModelSeconds": observed_seconds(model_timings, "executionDurationMs"),
        "toolCalls": metrics.get("toolCalls"),
        "toolErrors": metrics.get("toolErrors"),
        "observedToolSeconds": observed_seconds(tool_timings, "executionDurationMs"),
        "observedToolQueueSeconds": observed_seconds(tool_timings, "queueDurationMs"),
        "compactions": compactions if metrics or compactions else None,
        "trajectoryError": trace_error,
        "submissions": submissions,
        "artifacts": {name: str(path.relative_to(run)) for name, path in paths.items()
                      if path.is_dir() or (path.is_file() and path.stat().st_size)},
    }


def summarize(run, mode, repeats, runner_exit, require_trace, require_atif,
              fixture_reward, tasks, job_name, registry, runner, suite):
    selected = split_tasks(tasks)
    expected = len(selected) * repeats
    job = run / job_name
    build = read_json(run / "build.json")
    result = read_json(job / "result.json")
    lock = read_json(job / "lock.json")
    dataset = read_dataset(registry) if registry else {}
    entries = {entry["name"]: entry for entry in dataset.get("tasks", [])}
    trials = [trial_summary(run, directory, entries) for directory in sorted(job.glob("*__*"))
              if directory.is_dir()]
    expected_reward = 1.0 if mode == "oracle" else fixture_reward
    evidence_failure = None
    for trial in trials:
        if trial["score"] is None:
            continue
        if require_trace and not trial["modelCallsObserved"]:
            reason = "missing trajectory metadata"
        elif require_atif and "atif" not in trial["artifacts"]:
            reason = "missing ATIF trajectories"
        elif any(not item["path"] for item in trial["submissions"]):
            reason = "missing a required submission"
        else:
            continue
        trial["score"] = None
        evidence_failure = evidence_failure or reason
    scored = [trial for trial in trials if trial["score"] is not None]
    threshold = dataset.get("pass_threshold", 1.0)
    passed = sum(trial["score"] >= threshold for trial in scored)
    costs = [trial["estimatedCostUsd"] for trial in trials if trial["estimatedCostUsd"] is not None]
    total_cost = sum(costs) if len(costs) == expected else None
    cases = []
    for task in selected:
        attempts = [trial for trial in trials if trial["task"] == task]
        rewards = [trial["score"] for trial in attempts if trial["score"] is not None]
        cases.append({
            "task": task,
            "requestedTrials": repeats,
            "scoredTrials": len(rewards),
            "tracedTrials": sum(bool(trial["modelCallsObserved"]) for trial in attempts),
            "meanScore": sum(rewards) / len(rewards) if rewards else None,
            "passRate": sum(reward >= threshold for reward in rewards) / len(rewards) if rewards else None,
        })
    all_scored = (runner_exit == 0 and result.get("finished_at") is not None
                  and result.get("n_total_trials") == len(trials) == len(scored) == expected
                  and all(case["scoredTrials"] == repeats for case in cases))
    traced = sum(case["tracedTrials"] for case in cases)
    failure = None
    if evidence_failure:
        failure = f"Talos benchmark job is {evidence_failure}"
    elif not all_scored:
        failure = "Talos benchmark job did not score every trial"
    elif mode in ("fixture", "oracle") and any(trial["score"] != expected_reward for trial in scored):
        failure = f"Talos {mode} validation failed"
    summary = run / "summary.json"
    write_json(summary, {
        "schemaVersion": 1,
        "build": build or None,
        "mode": mode,
        "suite": suite,
        "suiteVersion": dataset.get("version"),
        "datasetRef": f"{dataset['source']}@{dataset['digest']}" if "source" in dataset else None,
        "platform": build.get("platform") or os.environ.get("TALOS_BENCH_PLATFORM"),
        "runner": runner,
        "runnerExitCode": runner_exit,
        "runnerSeconds": elapsed(result),
        "harbor": lock.get("harbor"),
        "harborExitCode": runner_exit if runner == "harbor" else None,
        "harborSeconds": elapsed(result) if runner == "harbor" else None,
        "requestedTrials": expected,
        "scoredTrials": len(scored),
        "meanScore": sum(trial["score"] for trial in scored) / len(scored) if scored else None,
        "passRate": passed / len(scored) if scored else None,
        "meanEstimatedCostUsd": total_cost / expected if total_cost is not None else None,
        "estimatedCostPerPassUsd": total_cost / passed if total_cost is not None and passed else None,
        "complete": failure is None,
        "failure": failure,
        "tracedTrials": traced,
        "cases": cases,
        "trials": trials,
    })
    if failure is not None:
        raise SystemExit(f"{failure}. See {summary}")
    print(f"Talos benchmark scored {len(scored)}/{expected} trials. See {summary}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest="command", required=True)
    listing = subcommands.add_parser("list")
    listing.add_argument("suites", type=Path)
    version = subcommands.add_parser("version")
    version.add_argument("registry", type=Path)
    dataset_ref = subcommands.add_parser("dataset-ref")
    dataset_ref.add_argument("registry", type=Path)
    select = subcommands.add_parser("select")
    select.add_argument("registry", type=Path)
    select.add_argument("tasks")
    submissions = subcommands.add_parser("submissions")
    submissions.add_argument("registry", type=Path)
    submissions.add_argument("tasks")
    build = subcommands.add_parser("build")
    build.add_argument("run", type=Path)
    build.add_argument("repo", type=Path)
    build.add_argument("binary", type=Path)
    build.add_argument("source", choices=("checkout", "provided"))
    build.add_argument("mode", choices=("fixture", "real"))
    build.add_argument("repeats", type=int)
    build.add_argument("--tasks", default="regex-log")
    summary = subcommands.add_parser("summary")
    summary.add_argument("run", type=Path)
    summary.add_argument("mode", choices=("fixture", "real", "oracle"))
    summary.add_argument("repeats", type=int)
    summary.add_argument("--runner-exit", "--harbor-exit", type=int, default=0)
    summary.add_argument("--runner", choices=("harbor", "pier"), default="harbor")
    summary.add_argument("--suite", default="terminal-bench")
    summary.add_argument("--require-trace", action="store_true")
    summary.add_argument("--require-atif", action="store_true")
    summary.add_argument("--fixture-reward", type=float, default=1.0)
    summary.add_argument("--tasks", default="regex-log")
    summary.add_argument("--job-name", default="regex-log")
    summary.add_argument("--registry", type=Path)
    args = vars(parser.parse_args())
    command = args.pop("command")
    if command == "list":
        list_suites(**args)
    elif command == "version":
        print(read_dataset(args["registry"])["version"])
    elif command == "dataset-ref":
        dataset = read_dataset(args["registry"])
        print(f"{dataset['source']}@{dataset['digest']}")
    elif command == "select":
        select_tasks(**args)
    elif command == "submissions":
        submission_paths(**args)
    elif command == "build":
        record_build(**args)
    else:
        summarize(**args)


if __name__ == "__main__":
    main()
