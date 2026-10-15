#!/usr/bin/env python3
"""Select checks from actual product changes for PRs, main pushes and hooks."""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parents[2]
DOCUMENT_SUFFIXES = (".md", ".mdx", ".rst", ".adoc")
SCOPES = {
    "rust": ("talos-rs/", "justfile"),
    "frontend": ("talos-app/",),
    "pr-e2e": ("conf/", "talos-app/", "talos-e2e/", "talos-rs/", "justfile"),
    "tui-install": (
        "conf/", "talos-rs/", "build/", "deploy/install/", "build.sh",
        "talos-e2e/scripts/common.sh", "talos-e2e/scripts/tui_install.sh",
        "talos-e2e/run_e2e.sh", "talos-e2e/scenarios/15_tui_install/",
        "talos-e2e/tests/tui_install_target_test.sh",
        "talos-e2e/docker/tui-install-ubuntu-24.04.Dockerfile",
    ),
}
CI_INPUTS = {
    "rust": ("scripts/check_error_contract*.py", ".github/workflows/talos-rust.yml"),
    "frontend": ("scripts/check_error_contract.py", ".github/workflows/talos-frontend-app.yml"),
    "pr-e2e": ("build.sh", "build/*", ".github/workflows/pr-e2e.yml"),
    "tui-install": (".github/workflows/talos-tui-install.yml",),
}
VERSION = r"[0-9]+\.[0-9]+\.[0-9]+(?:-SNAPSHOT)?"
VERSION_PATTERNS = {
    "talos-rs/Cargo.toml": rf'(?m)(^\[workspace\.package\]\n(?:(?!\[)[^\n]*\n)*?version = ")({VERSION})(")',
    "talos-rs/apps/tui/src/utils.rs": rf'(APP_VERSION: &str = "v)({VERSION})(")',
    "talos-app/ts-sdk/packages/app-server/src/stdio.ts": rf'(version: this\.options\.clientVersion \?\? ")({VERSION})(")',
    "deploy/docker/.env.example": rf'(?m)(^TALOS_SERVER_VERSION=v)({VERSION})($)',
    "deploy/docker/docker-compose.yml": rf'(TALOS_SERVER_VERSION:-v)({VERSION})(\}})',
}


def is_document(path: str) -> bool:
    return path.lower().endswith(DOCUMENT_SUFFIXES)


def scope_matches(scope: str, path: str, *, product_only: bool = False) -> bool:
    if is_document(path):
        return False
    if any(path.startswith(p) if p.endswith("/") else path == p for p in SCOPES[scope]):
        return True
    return not product_only and (
        path == ".github/release-toolchain.env"
        or any(fnmatch.fnmatchcase(path, pattern) for pattern in CI_INPUTS[scope])
    )


def should_run(scope: str, paths: list[str], *, product_only: bool = False) -> bool:
    return any(scope_matches(scope, path, product_only=product_only) for path in paths)


def version_files() -> set[str]:
    return set(subprocess.check_output(
        ["bash", str(ROOT / "scripts/check-release-version.sh"), "--files"], text=True
    ).splitlines())


def normalize_version(path: str, content: str) -> str | dict | None:
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
    paths = changed_paths(base, head) if base else []
    relevant = should_run(args.scope, paths, product_only=args.product_only) if base else True
    if args.print_result:
        print(str(relevant).lower())
        return 0
    if output := os.environ.get("GITHUB_OUTPUT"):
        with open(output, "a", encoding="utf-8") as stream:
            stream.write(f"run_workflow={str(relevant).lower()}\n")
    print(f"{args.scope}: {'run affected checks' if relevant else 'no product changes; metadata/policy checks apply'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
