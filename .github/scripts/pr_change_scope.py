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
import tomllib


ROOT = Path(__file__).resolve().parents[2]
DOCUMENT_SUFFIXES = (".md", ".mdx", ".rst", ".adoc")
E2E_ROUTING_INPUTS = (".github/workflows/pr-e2e.yml", ".github/scripts/pr_change_scope*.py")
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
    "pr-e2e": ("build.sh", "build/*", *E2E_ROUTING_INPUTS),
    "tui-install": (".github/workflows/talos-tui-install.yml",),
}
E2E_SCENARIOS = [
    {"name": "health-and-model-catalog", "runner": "ubuntu-24.04", "targets": "group:01_models"},
    {"name": "app-server", "runner": "ubuntu-24.04", "targets": "case:09_app_server/smoke_e2e\ncase:09_app_server/desktop_recovery"},
    {"name": "computer-registration", "runner": "ubuntu-24.04", "targets": "case:09_app_server/computer_deletion"},
    {"name": "files", "runner": "ubuntu-24.04", "targets": "group:12_files"},
    {"name": "sandbox", "runner": "ubuntu-24.04", "targets": "group:14_sandbox"},
]
VERSION = r"[0-9]+\.[0-9]+\.[0-9]+(?:-SNAPSHOT)?"
VERSION_PATTERNS = {
    "talos-rs/Cargo.toml": rf'(?m)(^\[workspace\.package\]\n(?:(?!\[)[^\n]*\n)*?version = ")({VERSION})(")',
    "talos-rs/apps/tui/src/utils.rs": rf'(APP_VERSION: &str = "v)({VERSION})(")',
    "talos-app/ts-sdk/packages/app-server/src/stdio.ts": rf'(version: this\.options\.clientVersion \?\? ")({VERSION})(")',
    "deploy/docker/talos-worker/.env.example": rf'(?m)(^TALOS_WORKER_VERSION=v)({VERSION})($)',
    "deploy/docker/talos-server/.env.example": rf'(?m)(^TALOS_SERVER_VERSION=v)({VERSION})($)',
    "deploy/docker/talos-server/docker-compose.yml": rf'(TALOS_SERVER_VERSION:-v)({VERSION})(\}})',
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


def e2e_product_paths(paths: list[str]) -> list[str]:
    inputs = [path for path in paths if scope_matches("pr-e2e", path)]
    product = [path for path in inputs if not any(
        fnmatch.fnmatchcase(path, pattern) for pattern in E2E_ROUTING_INPUTS
    )]
    # Always-on policy tests exercise routing/gate definitions. Their inclusion
    # must not broaden a bounded product diff; routing-only edits stay conservative.
    return product or inputs


def frontend_e2e_scope(paths: list[str]) -> str | None:
    paths = e2e_product_paths(paths)
    for kind, prefixes in (
        ("web", ("talos-app/web/src/", "talos-app/web/public/")),
        ("desktop", ("talos-app/desktop/src/", "talos-app/desktop/resources/")),
    ):
        if paths and all(path.startswith(prefixes) for path in paths):
            return kind
    return None


def e2e_plan(paths: list[str]) -> tuple[bool, dict]:
    frontend = frontend_e2e_scope(paths)
    if frontend == "desktop":
        # The dedicated Browser job owns native resources/host/MCP acceptance.
        return False, {"include": []}
    web_only = frontend == "web"
    scenarios = [s for s in E2E_SCENARIOS if s["name"] == "computer-registration"] if web_only else list(E2E_SCENARIOS)
    # Heavy Browser owners run the complete numbered group in its dedicated job.
    # Lightweight resource-only edits retain the real product scenario once.
    if web_only and not browser_e2e_applies(paths):
        scenarios = [*scenarios, {"name": "resource-workspace", "runner": "ubuntu-24.04",
                                  "targets": "case:20_browser_workspace/01_resource_tabs"}]
    return not web_only, {"include": scenarios}


def browser_e2e_applies(paths: list[str]) -> bool:
    """Keep Browser lifecycle coverage for its real owners without taxing unrelated Web edits."""
    if not paths:
        return True
    paths = e2e_product_paths(paths)
    web_owners = (
        "talos-app/web/src/features/computers/", "talos-app/web/src/features/settings/",
        "talos-app/web/src/app/", "talos-app/web/src/pages/chat/",
        "talos-app/web/src/widgets/chat-context-bar/", "talos-app/web/src/shared/api/",
        "talos-app/web/src/shared/store/", "talos-app/web/src/shared/ui/Button.",
        "talos-app/web/src/shared/i18n/desktop", "talos-app/web/src/shared/i18n/computer",
        "talos-app/web/src/shared/i18n/mcp", "talos-app/web/src/shared/i18n/apiError",
    )
    return any(not is_document(path) and (
        not path.startswith(("talos-app/web/src/", "talos-app/web/public/"))
        or path.startswith(web_owners)
        or path == "talos-app/web/public/talos-logo.svg"
    ) for path in paths)


def browser_e2e_plan(paths: list[str]) -> tuple[list[str], bool]:
    if not browser_e2e_applies(paths):
        return [], False
    if frontend_e2e_scope(paths) != "desktop":
        return ["group:20_browser_workspace"], True
    targets = [f"case:20_browser_workspace/{case}" for case in (
        "02_desktop_resources", "03_desktop_browser", "04_desktop_mcp",
    )]
    media_owners = tuple(f"talos-app/desktop/src/{name}" for name in (
        "desktop-media", "desktop-ipc", "desktop-server-requests", "ipc-guard",
        "ipc.", "preload.", "app-server-controller",
    ))
    docker = any(path.startswith(media_owners) for path in e2e_product_paths(paths))
    if docker:
        targets.append("case:20_browser_workspace/05_docker_worker")
    return targets, docker


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
    paths = changed_paths(base, head) if base else []
    relevant = should_run(args.scope, paths, product_only=args.product_only) if base else True
    if args.print_result:
        print(str(relevant).lower())
        return 0
    if output := os.environ.get("GITHUB_OUTPUT"):
        with open(output, "a", encoding="utf-8") as stream:
            if args.scope == "pr-e2e":
                native, matrix = e2e_plan(paths)
                targets, docker = browser_e2e_plan(paths)
                stream.write(f"run_browser={str(relevant and bool(targets)).lower()}\n")
                stream.write(f"browser_targets={' '.join(targets)}\n")
                stream.write(f"browser_docker={str(relevant and docker).lower()}\n")
                stream.write(f"run_scenarios={str(relevant and bool(matrix['include'])).lower()}\n")
                stream.write(f"run_contracts={str(relevant and native).lower()}\n")
                stream.write(f"scenario_matrix={json.dumps(matrix, separators=(',', ':'))}\n")
            stream.write(f"run_workflow={str(relevant).lower()}\n")
    print(f"{args.scope}: {'run affected checks' if relevant else 'no product changes; metadata/policy checks apply'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
