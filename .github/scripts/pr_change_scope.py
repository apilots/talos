#!/usr/bin/env python3
"""Select checks from actual product changes for PRs, main pushes and hooks."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tomllib


ROOT = Path(__file__).resolve().parents[2]
SCOPES = ("rust", "frontend", "pr-e2e", "tui-install", "ci")
from ci_impact import impact, is_document

VERSION = r"[0-9]+\.[0-9]+\.[0-9]+(?:-SNAPSHOT)?"
VERSION_PATTERNS = {
    "talos-rs/Cargo.toml": rf'(?m)(^\[workspace\.package\]\n(?:(?!\[)[^\n]*\n)*?version = ")({VERSION})(")',
    "talos-rs/apps/tui/src/utils.rs": rf'(APP_VERSION: &str = "v)({VERSION})(")',
    "talos-app/ts-sdk/packages/app-server/src/stdio.ts": rf'(version: this\.options\.clientVersion \?\? ")({VERSION})(")',
    "deploy/docker/talos-worker/.env.example": rf'(?m)(^TALOS_WORKER_VERSION=v)({VERSION})($)',
    "deploy/docker/talos-server/.env.example": rf'(?m)(^TALOS_SERVER_VERSION=v)({VERSION})($)',
    "deploy/docker/talos-server/docker-compose.yml": rf'(TALOS_SERVER_VERSION:-v)({VERSION})(\}})',
}


def should_run(scope: str, paths: list[str], *, product_only: bool = False) -> bool:
    if product_only:
        paths = [p for p in paths if p.startswith(("talos-rs/", "talos-app/", "conf/")) or p == "justfile"]
    selected = impact(paths)
    return {"rust": bool(selected.rust), "frontend": bool(selected.frontend),
            "pr-e2e": any(selected.targets.values()) or selected.harness,
            "tui-install": selected.installer,
            "ci": bool(selected.rust or selected.frontend or any(selected.targets.values())
                       or selected.installer or selected.harness)}[scope]


def version_files() -> set[str]:
    return set(subprocess.check_output(
        ["bash", str(ROOT / "scripts/check-release-version.sh"), "--files"], text=True
    ).splitlines())


def normalize_version(path: str, content: str) -> str | dict | None:
    if path == "talos-rs/Cargo.lock":
        data = tomllib.loads(content)
        local = [p for p in data.get("package", []) if "source" not in p]
        if not local or any(not re.fullmatch(VERSION, p["version"]) for p in local):
            return None
        references = {f'{p["name"]} {p["version"]}': f'{p["name"]} <version>' for p in local}
        for package in data["package"]:
            if "source" not in package:
                package["version"] = "<version>"
            if "dependencies" in package:
                package["dependencies"] = [references.get(dep, dep) for dep in package["dependencies"]]
        return data
    if path.endswith("/package.json"):
        data = json.loads(content)
        version = data.get("version")
        if not isinstance(version, str) or not re.fullmatch(VERSION, version):
            return None
        return {**data, "version": "<version>"}
    pattern = VERSION_PATTERNS[path]
    normalized, count = re.subn(pattern, r"\1<version>\3", content)
    return normalized if count == 1 else None


def version_change_only(path: str, before: str, after: str) -> bool:
    normalized = normalize_version(path, before)
    return normalized is not None and normalized == normalize_version(path, after)


def file_at(ref: str, path: str) -> str | None:
    entry = subprocess.check_output(["git", "ls-tree", ref, "--", path])
    if not entry:
        return None
    return subprocess.check_output(["git", "show", f"{ref}:{path}"], text=True)


def changed_paths(base: str, head: str) -> list[str]:
    paths = subprocess.check_output(
        ["git", "diff", "--name-only", "--no-renames", "-z", base, head], text=True
    ).rstrip("\0").split("\0")
    versions = version_files()
    relevant = []
    for path in filter(None, paths):
        if is_document(path):
            continue
        if path in versions:
            before, after = file_at(base, path), file_at(head, path)
            if before is not None and after is not None and version_change_only(path, before, after):
                continue
        relevant.append(path)
    return relevant


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scope", choices=SCOPES)
    parser.add_argument("--base")
    parser.add_argument("--head")
    parser.add_argument("--product-only", action="store_true", help="Local hook product checks.")
    parser.add_argument("--print-result", action="store_true", help="Print only true or false.")
    args = parser.parse_args()
    if bool(args.base) != bool(args.head):
        parser.error("--base and --head must be supplied together")
    event = os.environ.get("GITHUB_EVENT_NAME")
    base, head = args.base, args.head
    if not base and event == "pull_request":
        head = os.environ["PR_HEAD_SHA"]
        base = subprocess.check_output(
            ["git", "merge-base", os.environ["PR_BASE_SHA"], head], text=True
        ).strip()
    elif not base and event == "push":
        base, head = os.environ["PUSH_BASE_SHA"], os.environ["PUSH_HEAD_SHA"]
        if base == "0" * 40:
            base = subprocess.check_output(["git", "hash-object", "-w", "-t", "tree", "--stdin"],
                                           input="", text=True).strip()
    paths = changed_paths(base, head) if base else [".github/release-toolchain.env"]
    relevant = should_run(args.scope, paths, product_only=args.product_only)
    if args.print_result:
        print(str(relevant).lower())
        return 0
    values = {"run_workflow": str(relevant).lower()}
    if args.scope == "ci":
        from ci_plan import plan
        selected = plan(paths)
        values.update({"plan": json.dumps(selected, separators=(",", ":"))})
        values.update({category: str(bool(task["quality"] or task["scenarios"]["include"])).lower()
                       for category, task in selected["categories"].items()})
        values.update(installer=str(selected["installer"]).lower(),
                      build=str(bool(selected["build"]["binaries"])).lower())
        summary = os.environ.get("GITHUB_STEP_SUMMARY")
        if summary:
            with open(summary, "a") as stream:
                stream.write("## Selected module checks\n\n```json\n" + json.dumps(selected, indent=2) + "\n```\n")
    if output := os.environ.get("GITHUB_OUTPUT"):
        with open(output, "a", encoding="utf-8") as stream:
            values["run_workflow"] = values.pop("run_workflow")
            for key, value in values.items():
                stream.write(f"{key}={value}\n")
    print(f"{args.scope}: {'run affected checks' if relevant else 'no product changes; metadata/policy checks apply'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
