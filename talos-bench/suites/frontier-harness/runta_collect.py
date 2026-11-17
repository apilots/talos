"""Package a Runta batch's original job archives and an honest coverage index."""

import argparse
import hashlib
import json
from pathlib import Path
import zipfile


def sha256(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def collect(batch, destination):
    manifest = json.loads((batch / "manifest.json").read_text())
    rows = []
    with zipfile.ZipFile(destination, "x", compression=zipfile.ZIP_STORED) as package:
        for task in manifest["tasks"]:
            case = batch / "cases" / task.replace("/", "--")
            path = case / "state.json"
            row = {"task": task, "status": "not_started"}
            if path.is_file():
                state = json.loads(path.read_text())
                row.update({key: state.get(key) for key in ("status", "started_at", "finished_at", "exit_code",
                                                            "runtime", "complete", "scored_trials", "score")})
                result_dir = Path(state.get("output") or "")
                summary = result_dir / "summary.json"
                if state.get("output") and summary.is_file():
                    trials = json.loads(summary.read_text()).get("trials", [])
                    row["trial_evidence"] = [{key: trial.get(key) for key in
                                             ("trial", "score", "verifierReward", "exception", "stopReason")}
                                             for trial in trials]
                archive = result_dir / "original-job.tar.gz"
                if archive.is_file() and state.get("archive_sha256") == sha256(archive):
                    name = "jobs/" + task.replace("/", "--") + "/original-job.tar.gz"
                    package.write(archive, name)
                    row.update(archive=name, archive_sha256=state["archive_sha256"])
                else:
                    row["archive"] = None
                if (case / "launcher.log").is_file():
                    # Logs may contain local paths; retain them locally, not in the external bundle.
                    row["launcher_log_retained_locally"] = True
            rows.append(row)
        environment = batch / "environment.json"
        if environment.is_file():
            package.write(environment, "environment.json")
        public = {key: value for key, value in manifest.items() if key != "secret_name"}
        public["supplemental_evidence"] = []
        for name in ("verifier-diagnostic-evidence.tar.gz", "batched-regrade-evidence.tar.gz"):
            source = batch / name
            if source.is_file():
                target = "supplemental/" + name
                package.write(source, target)
                public["supplemental_evidence"].append({"path": target, "sha256": sha256(source)})
        public["cases"] = rows
        package.writestr("manifest.json", json.dumps(public, indent=2) + "\n")
    return rows


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("batch", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    rows = collect(args.batch, args.destination)
    print(json.dumps({"total": len(rows), "archived": sum(bool(x.get("archive")) for x in rows),
                      "complete": sum(x.get("complete") is True for x in rows)}, indent=2))


if __name__ == "__main__":
    main()
