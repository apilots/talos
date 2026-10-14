#!/usr/bin/env python3
"""Decide whether a pull request needs a resource-heavy quality workflow."""

from __future__ import annotations

import fnmatch
import os
import subprocess
import sys


DOCUMENT_SUFFIXES = (".md", ".mdx", ".rst", ".adoc")
DOCUMENT_PATHS = (
    "docs/**",
    "specs/**",
    ".github/ISSUE_TEMPLATE/**",
    ".github/PULL_REQUEST_TEMPLATE.md",
    "AGENTS.md",
    "CONTRIBUTING*.md",
)
SCOPES = {
    "pr-e2e": (
        "talos-rs/**",
        "talos-app/**",
        "talos-e2e/**",
        "conf/**",
        "deploy/**",
        "build/**",
        "build.sh",
        "justfile",
        "Cargo.toml",
        "pnpm-workspace.yaml",
        "pnpm-lock.yaml",
    ),
    "rust": (
        "talos-rs/**",
        "scripts/check_error_contract*.py",
        "justfile",
    ),
    "frontend": (
        "talos-app/**",
    ),
    "tui-install": (
        "build.sh",
        "build/scripts/verify-release-artifacts.sh",
        "conf/**",
        "deploy/install/**",
        "talos-e2e/lib/**",
        "talos-e2e/run_e2e.sh",
        "talos-e2e/scripts/tui_install.sh",
        "talos-e2e/scenarios/15_tui_install/**",
        "talos-e2e/tests/tui_install_target_test.sh",
        "talos-rs/Cargo.toml",
    ),
}


def is_document_only_path(path: str) -> bool:
    normalized = path.removeprefix("./")
    if any(fnmatch.fnmatchcase(normalized, pattern) for pattern in DOCUMENT_PATHS):
        return True
    return normalized.lower().endswith(DOCUMENT_SUFFIXES)


def changed_paths() -> list[str]:
    base = os.environ["PR_BASE_SHA"]
    head = os.environ["PR_HEAD_SHA"]
    result = subprocess.run(
        ["git", "diff", "--name-only", f"{base}...{head}"],
        check=True,
        capture_output=True,
        text=True,
    )
    return [path for path in result.stdout.splitlines() if path]


def scope_matches(scope: str, path: str) -> bool:
    if is_document_only_path(path):
        return False
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in SCOPES[scope])


def should_run(scope: str, paths: list[str]) -> bool:
    return any(scope_matches(scope, path) for path in paths)


def main() -> int:
    if len(sys.argv) != 2 or sys.argv[1] not in SCOPES:
        print(f"usage: {sys.argv[0]} <{', '.join(SCOPES)}>", file=sys.stderr)
        return 2

    scope = sys.argv[1]
    if os.environ.get("GITHUB_EVENT_NAME") != "pull_request":
        relevant = True
        paths: list[str] = []
    else:
        paths = changed_paths()
        relevant = should_run(scope, paths)

    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as stream:
            stream.write(f"run_workflow={'true' if relevant else 'false'}\n")

    if relevant:
        print(f"{scope}: relevant product or workflow inputs changed")
    else:
        print(f"{scope}: no relevant product inputs changed; skip resource-heavy jobs")
        if paths:
            print("Changed files:")
            for path in paths:
                print(f"  {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
