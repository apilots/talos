"""Record a benchmark build and summarize the Harbor job beside its raw artifacts."""

import argparse
from datetime import datetime
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess

from runtime.payload import cli_log_lines, run_metrics


USAGE_COUNTERS = ("input_tokens", "output_tokens", "cache_read_input_tokens", "cache_creation_input_tokens")


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
    if not usage or not prices:
        return None
    inputs, outputs, cached, written = (usage.get(key) for key in USAGE_COUNTERS)
    if any(type(count) is not int or count < 0 for count in (inputs, outputs, cached, written)):
        return None
    input_rate, output_rate, read_rate, write_rate = (
        prices.get(key) for key in ("input", "output", "cache_read", "cache_write")
    )
    if any(type(rate) not in (int, float) or not math.isfinite(rate) or rate < 0
           for rate in (input_rate, output_rate, read_rate, write_rate)):
        return None
    uncached = inputs - cached - written
    if uncached < 0:
        return None
    return (uncached * input_rate + cached * read_rate + written * write_rate + outputs * output_rate) / 1_000_000


def root_cache_read(calls, metrics, observed_requests):
    """Compare token-weighted cache reads from root call records with CLI totals."""
    observed_input = covered_input = cache_read = measured_calls = 0
    inconsistent = False
    for call in calls:
        usage = (call.get("metrics") or {}).get("usage") or {}
        inputs = usage.get("input_tokens")
        observed_input += inputs or 0
        cached = usage.get("cache_read_input_tokens")
        if cached is None:
            continue
        if inputs is None or not 0 <= cached <= inputs:
            inconsistent = True
            continue
        covered_input += inputs
        cache_read += cached
        measured_calls += 1

    totals = metrics.get("usage") or {}
    total_input = totals.get("input_tokens")
    total_cached = totals.get("cache_read_input_tokens")
    missing_rounds = (metrics.get("modelRounds") or 0) > observed_requests
    if total_input is not None:
        inconsistent |= observed_input > total_input
        if total_cached is not None:
            inconsistent |= (not cache_read <= total_cached <= total_input
                             or (covered_input == total_input and cache_read != total_cached))
    if inconsistent:
        status = "inconsistent"
    elif not covered_input:
        status = "unavailable"
    elif covered_input == total_input and cache_read == total_cached and not missing_rounds:
        status = "verified"
    else:
        status = "partial"
    return {
        "status": status,
        "ratio": cache_read / covered_input if status == "verified" else None,
        "observedCalls": len(calls),
        "callsWithReportedCacheRead": measured_calls,
        "coveredInputTokens": covered_input,
        "observedCacheReadTokens": cache_read,
    }


def workload_usage(snapshot, root_turn, root_cache, root_cost, cli):
    """Reconcile captured descendant Turns before reporting whole-workload metrics."""
    if not snapshot and cli.get("collaboration_tools") is not False:
        return None

    starts = set()
    root_starts = set()
    fields = snapshot.get("fields", ())
    for event in snapshot.get("events", ()):
        row = dict(zip(fields, event))
        if row.get("event") == "turnStarted":
            if row.get("path"):
                starts.add((row["path"], row["turnId"]))
            else:
                root_starts.add(row["turnId"])

    metrics = {(row["path"], row["turnId"]): row
               for row in snapshot.get("observedTurnMetrics", ()) if row["path"] and row["usage"]}
    calls = {(row["path"], row["turnId"]): row for row in snapshot.get("childUsage", ())}
    descendants = starts | metrics.keys() | calls.keys()
    reconciled = {}
    for key in metrics.keys() & calls.keys():
        reported, evidence = metrics[key], calls[key]
        usage = reported["usage"]
        inputs, reads = usage.get("input_tokens"), usage.get("cache_read_input_tokens")
        if (type(inputs) is int and type(reads) is int and 0 <= reads <= inputs
                and type(reported.get("modelRounds")) is int
                and evidence["observedCalls"] >= reported["modelRounds"]
                and evidence["observedCalls"] == evidence["callsWithReportedCacheRead"]
                and inputs == evidence["coveredInputTokens"] == evidence["reportedInputTokens"]
                and reads == evidence["observedCacheReadTokens"] == evidence["reportedCacheReadTokens"]):
            reconciled[key] = usage

    # Spawn and follow-up inherit the selected root model in an isolated Bench trial.
    rates = cli.get("prices_per_million") if cli.get("model") else None
    priced = [cost for usage in reconciled.values() if (cost := estimate_cost(usage, rates)) is not None]
    observed = {
        key: sum(values) if values and all(type(value) is int and value >= 0 for value in values) else None
        for key in USAGE_COUNTERS
        if (values := [metric["usage"].get(key) for metric in metrics.values()])
    }
    descendants_complete = (cli.get("collaboration_tools") is False or (
        "childUsage" in snapshot and root_turn is not None and root_starts == {root_turn}
        and snapshot.get("activeTurns") == [] and descendants == starts
        and reconciled.keys() == descendants
    ))
    settled = cli.get("exit_code") == 0 and descendants_complete
    cache_complete = settled and (root_cache or {}).get("status") == "verified"
    cache_ratio = None
    if cache_complete:
        inputs = root_cache["coveredInputTokens"] + sum(usage["input_tokens"] for usage in reconciled.values())
        reads = root_cache["observedCacheReadTokens"] + sum(usage["cache_read_input_tokens"] for usage in reconciled.values())
        cache_ratio = reads / inputs
    return {
        "descendantTurnsObserved": len(descendants),
        "descendantTurnsWithUsage": len(metrics),
        "descendantTurnsReconciled": len(reconciled),
        "observedDescendantUsage": observed or None,
        "pricedDescendantTurns": len(priced),
        "reconciledDescendantEstimatedCostUsd": sum(priced) if priced else None,
        "cacheReadRatio": cache_ratio,
        "estimatedCostUsd": root_cost + sum(priced)
        if settled and root_cost is not None and len(priced) == len(descendants) else None,
    }


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
    requests = [entry["request"] for entry in trace.get("data") or [] if entry["type"] == "modelRequest"]
    calls = [record for request in requests for record in request["callRecords"]]
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
    delegation_events = agent / "talos-delegation-events.json"
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
    provenance = read_json(run / "provenance" / f"{task_name}.json")
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
        "delegationEvents": delegation_events,
        "stdout": agent / "talos.stdout.txt",
        "stderr": agent / "talos.stderr.txt",
        "verifier": directory / "verifier",
        "submissionManifest": archive / "manifest.json",
    }
    cache = root_cache_read(calls, metrics, len(requests)) if usage is not None or calls else None
    root_cost = estimate_cost(usage, cli.get("prices_per_million"))
    workload = workload_usage(read_json(delegation_events), (trace.get("identity") or {}).get("turnId"),
                              cache, root_cost, cli)
    return {
        "trial": directory.name,
        "task": task_name,
        "taskRevision": task.get("git_commit_id") or entry.get("git_commit_id") or provenance.get("datasetRevision"),
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
        "rootCacheRead": cache,
        "estimatedCostUsd": root_cost,
        "workloadUsage": workload,
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
    workload_costs = [cost for trial in trials
                      if (cost := (trial["workloadUsage"] or {}).get("estimatedCostUsd")) is not None]
    workload_total = sum(workload_costs) if len(workload_costs) == expected else None
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
        "workloadCostCoveredTrials": len(workload_costs),
        "meanWorkloadEstimatedCostUsd": workload_total / expected if workload_total is not None else None,
        "workloadEstimatedCostPerPassUsd": workload_total / passed if workload_total is not None and passed else None,
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
