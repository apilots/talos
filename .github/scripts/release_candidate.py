#!/usr/bin/env python3
"""Bind the successful release-branch source gate and lock to formal builds."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path


def gh_api(path: str) -> dict:
    return json.loads(subprocess.check_output(["gh", "api", path], text=True))


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def validate_run(run: dict, repository: str, sha: str) -> None:
    if not (
        run["repository"]["full_name"] == repository
        and run["head_repository"]["full_name"] == repository
        and run["head_sha"] == sha
        and run["path"] == ".github/workflows/release.yml"
        and run["event"] == "push"
        and re.fullmatch(r"release-\d+\.\d+\.\d+", run["head_branch"])
        and run["status"] == "completed"
        and run["conclusion"] == "success"
    ):
        raise ValueError("Candidate must be a successful exact-source release-branch run")


def select_run(runs: list[dict], repository: str, sha: str) -> dict:
    for run in runs:
        if re.fullmatch(r"release-\d+\.\d+\.\d+", run["head_branch"] or ""):
            validate_run(run, repository, sha)
            return run
    raise ValueError("No completed candidate for this source; run the release-branch gate first")


def candidate_artifact(repository: str, run: dict) -> dict:
    artifacts = gh_api(f"repos/{repository}/actions/runs/{run['id']}/artifacts")["artifacts"]
    name = f"release-candidate-{run['run_attempt']}"
    matches = [a for a in artifacts if a["name"] == name and not a["expired"]]
    if len(matches) != 1:
        raise ValueError("Candidate evidence is unavailable; rerun the release-branch gate")
    return matches[0]


def validate_evidence(evidence: dict, run: dict, directory: Path) -> None:
    previous_tag = evidence["previous_tag"]
    if not re.fullmatch(r"v\d+\.\d+\.\d+", previous_tag):
        raise ValueError("Candidate must identify its preceding published tag")
    expected = {
        "repository": run["repository"]["full_name"],
        "source_sha": run["head_sha"],
        "run_id": run["id"],
        "run_attempt": run["run_attempt"],
        "version": run["head_branch"].removeprefix("release-"),
        "lock_sha256": digest(directory / "Cargo.lock"),
        "toolchain_sha256": digest(Path(".github/release-toolchain.env")),
        "previous_tag": previous_tag,
        "previous_sha": subprocess.check_output(
            ["git", "rev-parse", f"{previous_tag}^{{commit}}"], text=True).strip(),
        "notes_sha256": digest(directory / "release-body.md"),
    }
    if evidence != expected or any(type(evidence.get(k)) is not type(v) for k, v in expected.items()):
        raise ValueError("Candidate source, run, version, lock or toolchain does not match")
    generated = subprocess.check_output(
        ["bash", "scripts/generate-release-notes.sh", expected["version"],
         "--from", previous_tag, "--to", expected["source_sha"]])
    if generated != (directory / "release-body.md").read_bytes():
        raise ValueError("Candidate Notes must equal the standard generator output")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=("record", "find", "verify"))
    parser.add_argument("--repository", required=True)
    parser.add_argument("--sha", required=True)
    parser.add_argument("--run-id", type=int)
    parser.add_argument("--run-attempt", type=int)
    parser.add_argument("--version")
    parser.add_argument("--previous-tag")
    parser.add_argument("--directory", type=Path)
    parser.add_argument("--outputs", type=Path)
    args = parser.parse_args()
    if not re.fullmatch(r"[\w.-]+/[\w.-]+", args.repository):
        parser.error("Invalid repository")
    if not re.fullmatch(r"[0-9a-f]{40}", args.sha):
        parser.error("Expected exact source SHA")

    if args.operation == "record":
        if not all((args.directory, args.run_id, args.run_attempt, args.version, args.previous_tag)):
            parser.error("record requires directory, run-id, run-attempt, version and previous-tag")
        if subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip() != args.sha:
            raise ValueError("Source checkout does not match candidate")
        args.directory.mkdir(parents=True, exist_ok=True)
        lock = args.directory / "Cargo.lock"
        lock.write_bytes(Path("talos-rs/Cargo.lock").read_bytes())
        evidence = dict(repository=args.repository, source_sha=args.sha,
                        run_id=args.run_id, run_attempt=args.run_attempt,
                        version=args.version, lock_sha256=digest(lock),
                        toolchain_sha256=digest(Path(".github/release-toolchain.env")),
                        previous_tag=args.previous_tag,
                        previous_sha=subprocess.check_output(
                            ["git", "rev-parse", f"{args.previous_tag}^{{commit}}"],
                            text=True).strip(),
                        notes_sha256=digest(args.directory / "release-body.md"))
        (args.directory / "candidate.json").write_text(json.dumps(evidence, indent=2) + "\n")
    elif args.operation == "find":
        runs = gh_api(f"repos/{args.repository}/actions/workflows/release.yml/runs"
                      f"?head_sha={args.sha}&event=push&per_page=100")["workflow_runs"]
        run = select_run(runs, args.repository, args.sha)
        artifact = candidate_artifact(args.repository, run)
        print(run["id"])
        if args.outputs:
            with args.outputs.open("a") as stream:
                stream.write(f"run_id={run['id']}\nartifact_id={artifact['id']}\n"
                             f"artifact_name={artifact['name']}\n")
    else:
        if not args.directory or not args.run_id:
            parser.error("verify requires directory and run-id")
        run = gh_api(f"repos/{args.repository}/actions/runs/{args.run_id}")
        validate_run(run, args.repository, args.sha)
        candidate_artifact(args.repository, run)
        if subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip() != args.sha:
            raise ValueError("Source checkout does not match candidate")
        validate_evidence(json.loads((args.directory / "candidate.json").read_text()),
                          run, args.directory)
        if args.outputs:
            with args.outputs.open("a") as stream:
                stream.write(f"lock_sha256={digest(args.directory / 'Cargo.lock')}\n")
        print(f"Validated candidate run {args.run_id}, source {args.sha}")


if __name__ == "__main__":
    main()
