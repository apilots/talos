#!/usr/bin/env python3
"""Select product CI jobs from the repository's top-level ownership folders."""

from __future__ import annotations

import os
import subprocess
import sys


DOCUMENT_SUFFIXES = (".md", ".mdx", ".rst", ".adoc")
SCOPES = {
    "rust": ("talos-rs/",),
    "frontend": ("talos-app/",),
    "pr-e2e": ("conf/", "talos-app/", "talos-e2e/", "talos-rs/"),
    "tui-install": (
        "conf/",
        "talos-rs/",
        "talos-e2e/scenarios/15_tui_install/",
    ),
}


def is_document(path: str) -> bool:
    return path.lower().endswith(DOCUMENT_SUFFIXES)


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
    if is_document(path):
        return False
    return any(path.startswith(directory) for directory in SCOPES[scope])


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
        print(f"{scope}: matching product directory changed")
    else:
        print(f"{scope}: no matching product directory changed; skip product jobs")
        if paths:
            print("Changed files:")
            for path in paths:
                print(f"  {path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
