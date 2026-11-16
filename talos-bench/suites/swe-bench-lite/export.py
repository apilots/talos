"""Stage a complete real-model SWE-bench Lite run for upstream packaging."""

import argparse
import json
from pathlib import Path
import shutil


def export(run, output):
    run = run.resolve()
    summary = json.loads((run / "summary.json").read_text())
    if summary["suite"] != "swe-bench-lite" or summary["mode"] != "real" or not summary["complete"]:
        raise SystemExit("Export requires a complete real-model SWE-bench Lite run.")
    trials = summary["trials"]
    instances = {trial["task"] for trial in trials}
    if len(instances) != len(trials):
        raise SystemExit("Export requires one attempt per SWE-bench instance.")

    files = {}
    for trial in trials:
        instance = trial["task"]
        if Path(instance).name != instance or instance == "..":
            raise SystemExit(f"Invalid SWE-bench instance ID: {instance}")
        logs = Path("logs") / instance
        patch, = trial["submissions"]
        patch_file = run / patch["path"]
        verifier = run / trial["artifacts"]["verifier"]
        transcript = trial["artifacts"].get("transcript")
        if not transcript:
            raise SystemExit(f"Missing recorded inference transcript for {instance}.")
        files.update({
            logs / "patch.diff": patch_file,
            logs / "report.json": verifier / "swe-bench-report.json",
            logs / "test_output.txt": verifier / "swe-bench-test-output.txt",
            Path("trajs") / f"{instance}.json": run / transcript,
        })
    for source in files.values():
        if not source.is_file() or not source.resolve().is_relative_to(run):
            raise SystemExit(f"Missing run artifact: {source}")
    predictions = [{
        "instance_id": trial["task"],
        "model_name_or_path": trial["model"],
        "model_patch": files[Path("logs") / trial["task"] / "patch.diff"].read_text(errors="surrogateescape"),
    } for trial in trials]
    output.mkdir(parents=True)
    for path, source in files.items():
        destination = output / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
    (output / "all_preds.jsonl").write_text("".join(json.dumps(row) + "\n" for row in predictions))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("run", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    export(args.run, args.output)
