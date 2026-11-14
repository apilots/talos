"""Run admitted cases from the pinned FrontierHarness native preflight."""

import argparse
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import tomllib
import traceback

from dotenv import dotenv_values
import yaml

BENCH = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(BENCH))

from summarize import artifact_identity, read_json, record_build, summarize, task_entries, write_json
from prebuild import PLATFORM, native_image, task_source
from prepare_native import copy_for_native_verifier, copy_with_prepared_images
from preflight import native as is_native_platform

DEFAULT_MODEL = "openai/gpt-6.1-terra"
MODEL_FAILURES = {"AgentTimeoutError", "AgentSafetyRefusalError"}


def check_prepared_image(receipt, task_ref, role):
    if (receipt["taskRef"] != task_ref or receipt["platform"] != PLATFORM
            or native_image(receipt["image"]) != receipt["imageId"]):
        raise ValueError(f"Prepared {role} image does not match {task_ref}")


def prepare(task, case, dataset_ref, prepared=None):
    name = task["name"]
    task_ref = f"{name}@{task['digest']}"
    source = task_source(task)
    config = tomllib.loads((source / "task.toml").read_text())
    verifier = task["verifier"]
    verifier_image = config.get("verifier", {}).get("environment", {}).get("docker_image")
    rebuild_verifier = (verifier.get("mode") == "separate" and verifier_image
                        and not any(map(is_native_platform, verifier["imagePlatforms"])))
    build_agent = not any(map(is_native_platform, task["agent"]["imagePlatforms"]))
    short_name = name.rsplit("/", 1)[-1]
    expected_name = name
    job_args = ["--task", task_ref]
    if prepared or rebuild_verifier:
        local_task = case / "native-task" / short_name
        provenance = {"datasetRef": dataset_ref, "taskRef": task_ref, "upstreamVerifierImage": verifier_image}
        compose_override = None
        if prepared:
            roles = ("agent", "verifier") if rebuild_verifier else ("agent",)
            images = {}
            for role in roles:
                receipt = json.loads((prepared / short_name / f"{role}.json").read_text())
                check_prepared_image(receipt, task_ref, role)
                images[role] = receipt
            if compose := task["agent"].get("composeServices"):
                services = images["agent"]["services"]
                if services.keys() != compose.keys() or services["main"]["imageId"] != images["agent"]["imageId"]:
                    raise ValueError(f"Prepared Compose services do not match {task_ref}")
                for service, receipt in services.items():
                    check_prepared_image(receipt, task_ref, service)
                compose_override = case / "native-services.yaml"
                compose_override.write_text(yaml.safe_dump({
                    "services": {service: {"image": receipt["image"], "platform": PLATFORM}
                                 for service, receipt in services.items()}
                }))
            adapted_digest = copy_with_prepared_images(source, local_task, {
                role: receipt["image"] for role, receipt in images.items()
            })
            provenance["preparedImages"] = images
            build_agent = False
        else:
            adapted_digest = copy_for_native_verifier(source, local_task, verifier_image)
        provenance["localTaskDigest"] = adapted_digest
        write_json(case / "provenance.json", provenance)
        job_args = ["--path", str(local_task)]
        if compose_override:
            job_args += ["--extra-docker-compose", str(compose_override)]
        expected_name = short_name
    if build_agent:
        job_args += ["--force-build", "--extra-docker-compose", str(BENCH / "runtime/native.compose.yaml")]
    submissions = list(config.get("artifacts", []))
    declared = {artifact_identity(item) for item in submissions}
    registered = task_entries(Path(__file__).with_name("registry.json"))
    for source_path in registered.get(name, {}).get("submission_paths", []):
        if artifact_identity(source_path) not in declared:
            submissions.append(source_path)
            job_args += ["--artifact", source_path]
    dataset, dataset_digest = dataset_ref.split("@", 1)
    registry = case / "registry.json"
    write_json(registry, [{"name": "frontier-harness", "version": "1.1.0", "source": dataset,
                           "digest": dataset_digest, "tasks": [{"name": expected_name,
                           "submission_paths": submissions}]}])
    return expected_name, registry, job_args


def failure_details(case, trial):
    paths = trial.get("artifacts", {})
    details = []
    sources = []
    if trial.get("exception") or trial.get("score") is None:
        sources = [case / "runner.log", *(case / paths[key] for key in ("trialLog", "stderr") if key in paths)]
    verifier = paths.get("verifier")
    verifier_stdout = None
    if verifier:
        directory = case / verifier
        verifier_stdout = directory / "test-stdout.txt"
        sources.append(verifier_stdout)
        try:
            ctrf = read_json(directory / "ctrf.json").get("results", {}).get("tests", [])
            details += [f"failedTest: {item['name']}" for item in ctrf if item["status"] in {"failed", "fail"}]
            reward_details = read_json(directory / "reward-details.json") or read_json(directory / "reward_details.json")
            criteria = (reward_details.get("reward") or {}).get("criteria", [])
            details += [f"failedCriterion: {item['name']}" for item in criteria if item.get("value") == 0]
            raw, threshold = reward_details.get("combined_raw"), reward_details.get("threshold")
            if type(raw) in (int, float) and type(threshold) in (int, float) and raw < threshold:
                details.append(f"compositeReward: {raw:.6g} below {threshold:.6g}")
        except (json.JSONDecodeError, AttributeError, KeyError):
            details.append("verifierDetailFormatUnreadable")
    details = [detail for detail in details if re.fullmatch(r"[A-Za-z0-9_: ./\-\[\]]{1,150}", detail)]
    tail = []
    for path in sources:
        if path.is_file():
            with path.open("rb") as stream:
                stream.seek(max(0, path.stat().st_size - 131072))
                output = stream.read().decode(errors="replace").lower()
                tail.append(output)
                if path == verifier_stdout:
                    uv_git_failure = ("failed to resolve `--with` requirement" in output
                                      and "git operation failed" in output)
                    uv_download_timeout = ("failed to download distribution due to network timeout" in output
                                           and "uv_http_timeout" in output)
                    if "uvx: command not found" in output or uv_git_failure or uv_download_timeout:
                        details.append("verifierDependencyBootstrap")
                    if "unable to obtain driver for chrome" in output:
                        details.append("verifierBrowserDriverUnavailable")
                    if "early eof" in output and "invalid index-pack output" in output:
                        details.append("verifierFixtureCloneFailed")
    log = "\n".join(tail)
    signals = {
        "no space left on device": "dockerStorageExhausted",
        "no matching manifest": "imagePlatformUnavailable",
        "exec format error": "nonNativeExecutable",
        "could not resolve host": "dependencyDnsFailure",
        "temporary failure resolving": "dependencyDnsFailure",
        "certificate verify failed": "dependencyTlsFailure",
        "ssl_error_syscall": "dependencyTlsFailure",
        "tls handshake timeout": "dependencyTlsFailure",
        "e: failed to fetch": "dependencyDownloadFailed",
        "failed to resolve source metadata": "registryManifestRequestFailed",
        "model response stream failure": "providerStreamFailure",
        "openai http request timed out": "providerRequestTimeout",
        "openai http request failed": "providerRequestFailed",
        "the provider rate limit was exceeded": "providerRateLimit",
        "the provider rate or usage limit was exceeded": "providerRateLimit",
        "permission denied": "permissionDenied",
    }
    return list(dict.fromkeys(details[:12] + [reason for signal, reason in signals.items() if signal in log]))


def outcome_from_summary(case, name, summary_path, elapsed):
    summary = read_json(summary_path)
    trial = next(iter(summary.get("trials", [])), {})
    score = trial.get("score")
    exception = trial.get("exception")
    if score is not None and summary.get("complete"):
        status = "scored"
        reason = "verifierRewardBelowOne" if score < 1 else None
    elif score is not None or (trial.get("verifierReward") is not None and not exception and not trial.get("stopReason")):
        status, score, reason = "infraFailure", None, "incompleteArtifacts"
    elif exception in MODEL_FAILURES:
        status, score, reason = "modelFailure", 0, exception
    else:
        status, reason = "infraFailure", exception or "runnerDidNotScore"
    details = failure_details(case, trial)
    bootstrap_failure = next((item for item in details if item in {
        "verifierDependencyBootstrap", "verifierFixtureCloneFailed", "verifierBrowserDriverUnavailable",
    }), None)
    if status == "scored" and bootstrap_failure:
        status, score, reason = "infraFailure", None, bootstrap_failure
    return {
        "name": name, "status": status, "reward": score, "elapsedSeconds": elapsed,
        "reason": reason, "summaryPath": str(summary_path) if summary_path.is_file() else None,
        "runnerLog": str(case / "runner.log"), "officialReward": trial.get("verifierReward"),
        "details": details,
    }


def run_case(task, root, dataset_ref, product, environment, prepared, runtime_args, model=DEFAULT_MODEL):
    name = task["name"]
    case = root / "cases" / name.rsplit("/", 1)[-1]
    try:
        case.mkdir(parents=True)
    except FileExistsError:
        print(f"{name}: preserved an existing case directory", flush=True)
        return
    start = time.monotonic()
    summary_path = case / "summary.json"
    failure = None
    with (case / "runner.log").open("w") as log:
        try:
            expected_name, registry, arguments = prepare(task, case, dataset_ref, prepared)
            record_build(case, BENCH.parent, product / "bin/talos", "provided", "real", 1, expected_name)
            result = subprocess.run([
                str(Path(sys.executable).with_name("harbor")), "run", "--env", "docker", "--yes",
                "--agent", "harbor_talos:AtifTalosAgent", "--model", model,
                "--agent-kwarg", f"product={product}", "--jobs-dir", str(case),
                "--job-name", "job", "--n-attempts", "1", "--n-concurrent", "1",
                *arguments, *runtime_args,
            ], env=environment, stdout=log, stderr=subprocess.STDOUT)
            try:
                summarize(case, "real", 1, result.returncode, True, True, None, expected_name,
                          "job", registry, "harbor", "frontier-harness")
            except SystemExit as error:
                print(error, file=log)
        except Exception:
            traceback.print_exc(file=log)
            failure = "batchPreparationOrSummaryFailed"
    outcome = outcome_from_summary(case, name, summary_path, time.monotonic() - start)
    if failure and not summary_path.is_file():
        outcome["reason"] = failure
    write_json(case / "outcome.json", outcome)
    print(f"{name}: {outcome['status']} ({outcome['reward']})", flush=True)


def memory(task):
    return max(task[role].get("memoryMb") or 2048 for role in ("agent", "verifier"))


def has_prepared_compose(task, prepared, selected):
    if (not prepared or not selected or task["name"] not in selected or task["status"] != "unresolved"
            or not (task.get("agent") or {}).get("composeServices")):
        return False
    name = task["name"].rsplit("/", 1)[-1]
    return bool(read_json(prepared / name / "agent.json").get("services"))


def run_batches(tasks, jobs, memory_mb, root, dataset_ref, product, environment, prepared, runtime_args, model):
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        active = {}
        while tasks or active:
            available = memory_mb - sum(active.values())
            index = next((i for i, task in enumerate(tasks) if memory(task) <= available), None)
            if index is not None and len(active) < jobs:
                task = tasks.pop(index)
                print(f"Starting {task['name']} ({memory(task)} MiB)", flush=True)
                future = pool.submit(run_case, task, root, dataset_ref, product, environment, prepared, runtime_args, model)
                active[future] = memory(task)
            elif active:
                finished, _ = wait(active, return_when=FIRST_COMPLETED)
                for future in finished:
                    del active[future]
                    future.result()
            else:
                raise ValueError("An admitted case exceeds the configured batch memory budget")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("preflight", type=Path)
    parser.add_argument("--product", type=Path, required=True)
    parser.add_argument("--dotenv", type=Path, required=True)
    parser.add_argument("--model", default=DEFAULT_MODEL, help="OpenAI model in provider/model form")
    parser.add_argument("--jobs", type=int, default=2)
    parser.add_argument("--memory-mb", type=int, default=10000)
    parser.add_argument("--task", action="append", help="Run only these qualified task names")
    parser.add_argument("--prepared-images", type=Path, help="Use prebuilt native images with pinned preparation receipts")
    parser.add_argument("--agent-env", action="append", default=[], help="Pass KEY=VALUE to Harbor's agent")
    parser.add_argument("--verifier-env", action="append", default=[], help="Pass KEY=VALUE to Harbor's verifier")
    options = parser.parse_args()
    if options.jobs < 1 or options.memory_mb < 1:
        parser.error("Jobs and memory budget must be positive")
    if not options.model.startswith("openai/") or not options.model.removeprefix("openai/"):
        parser.error("--model must select an OpenAI model as openai/<id>")
    native = subprocess.check_output(["docker", "version", "--format", "{{.Server.Os}}/{{.Server.Arch}}"], text=True).strip()
    inventory = json.loads(options.preflight.read_text())
    if native != PLATFORM or inventory["platform"] != native:
        parser.error("The inventory and Docker daemon must both use native Linux ARM64")
    os.environ["TALOS_BENCH_PLATFORM"] = native
    root = options.preflight.resolve().parent
    tasks = [task for task in inventory["tasks"]
             if (task["status"] in {"ready", "needsNativeBuild"}
                 or has_prepared_compose(task, options.prepared_images, options.task))
             and (not options.task or task["name"] in options.task)
             and not (root / "cases" / task["name"].rsplit("/", 1)[-1]).exists()]
    deferred = [task["name"] for task in tasks if memory(task) > options.memory_mb]
    if deferred:
        print(f"Deferred by batch memory budget: {', '.join(deferred)}", flush=True)
        tasks = [task for task in tasks if memory(task) <= options.memory_mb]
    if not tasks:
        print("No pending admitted cases")
        return
    key = dotenv_values(options.dotenv).get("TALOS_LLM_OPENAI_API_KEY")
    if not key:
        parser.error("The selected dotenv does not define TALOS_LLM_OPENAI_API_KEY")
    environment = dict(os.environ, OPENAI_API_KEY=key, TALOS_BENCH_PLATFORM=native,
                       DOCKER_DEFAULT_PLATFORM=native, HARBOR_TELEMETRY="0",
                       PYTHONPATH=str(BENCH / "runtime"))
    runtime_args = [arg for name, values in (("--agent-env", options.agent_env), ("--verifier-env", options.verifier_env))
                    for value in values for arg in (name, value)]
    run_batches(tasks, options.jobs, options.memory_mb, root, inventory["datasetRef"],
                options.product.resolve(), environment, options.prepared_images, runtime_args, options.model)


if __name__ == "__main__":
    main()
