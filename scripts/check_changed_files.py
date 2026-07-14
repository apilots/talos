#!/usr/bin/env python3
"""Validate only files added or modified in a staged or revision range."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


DEFAULT_MAX_BLOB_BYTES = 500 * 1024
FRONTEND_GENERIC_LINE_LIMIT = 1000
FRONTEND_TEST_LINE_LIMIT = 300
RUST_PRODUCTION_TARGET_LINES = 500
RUST_PRODUCTION_LINE_LIMIT = 800
RUST_TEST_LINE_LIMIT = 1200


@dataclass(frozen=True)
class ChangeSource:
    cached: bool
    base: str | None
    head: str | None


@dataclass(frozen=True)
class ChangedPath:
    path: str
    base_path: str | None


def run_git(*args: str, check: bool = True) -> bytes:
    result = subprocess.run(
        ["git", *args],
        check=check,
        capture_output=True,
    )
    return result.stdout


def parse_name_status(output: bytes) -> list[ChangedPath]:
    tokens = [token for token in output.split(b"\0") if token]
    changes: list[ChangedPath] = []
    index = 0
    while index < len(tokens):
        status = tokens[index].decode("utf-8")
        index += 1
        if status.startswith(("R", "C")):
            old_path = tokens[index].decode("utf-8")
            new_path = tokens[index + 1].decode("utf-8")
            index += 2
            changes.append(ChangedPath(path=new_path, base_path=old_path))
        else:
            path = tokens[index].decode("utf-8")
            index += 1
            changes.append(
                ChangedPath(path=path, base_path=path if status == "M" else None)
            )
    return sorted(changes, key=lambda change: change.path)


def changed_paths(source: ChangeSource) -> list[ChangedPath]:
    args = ["diff"]
    if source.cached:
        args.append("--cached")
    else:
        assert source.base is not None and source.head is not None
        args.extend([source.base, source.head])
    args.extend(["--name-status", "--diff-filter=AMCR", "--find-renames", "-z"])
    return parse_name_status(run_git(*args))


def object_content(revision: str, path: str) -> bytes | None:
    result = subprocess.run(
        ["git", "show", f"{revision}:{path}"],
        check=False,
        capture_output=True,
    )
    return result.stdout if result.returncode == 0 else None


def current_content(source: ChangeSource, path: str) -> bytes | None:
    revision = "" if source.cached else source.head
    assert revision is not None
    return object_content(revision, path)


def base_content(source: ChangeSource, path: str | None) -> bytes | None:
    if path is None:
        return None
    revision = "HEAD" if source.cached else source.base
    assert revision is not None
    return object_content(revision, path)


def count_non_empty_lines(content: bytes) -> int | None:
    try:
        text = content.decode("utf-8")
    except UnicodeDecodeError:
        return None
    return sum(1 for line in text.splitlines() if line.strip())


def frontend_line_limit(path: str) -> tuple[int, str] | None:
    if not path.startswith("talos-app/") or not path.endswith((".ts", ".tsx")):
        return None
    if re.search(r"\.(test|spec)\.tsx?$", path):
        return FRONTEND_TEST_LINE_LIMIT, "frontend test"
    if re.search(r"/components/.+\.tsx$", path):
        return 200, "frontend component"
    if re.search(r"/pages/.+\.tsx$", path):
        return 300, "frontend page"
    if re.search(r"/hooks/.+\.tsx?$", path):
        return 300, "frontend hook"
    if re.search(r"/(store\.ts|store/[^/]+\.tsx?)$", path):
        return 300, "frontend store"
    return FRONTEND_GENERIC_LINE_LIMIT, "frontend source"


def rust_line_limit(path: str) -> tuple[int, str] | None:
    if not path.startswith("talos-rs/") or not path.endswith(".rs"):
        return None
    name = Path(path).name
    is_test = (
        "/tests/" in path
        or "/test_support/" in path
        or name.endswith("_tests.rs")
        or name.endswith("_test.rs")
    )
    if is_test:
        return RUST_TEST_LINE_LIMIT, "Rust test"
    return RUST_PRODUCTION_LINE_LIMIT, "Rust source"


def line_limit(path: str) -> tuple[int, str] | None:
    return frontend_line_limit(path) or rust_line_limit(path)


def line_limit_error(
    path: str,
    label: str,
    limit: int,
    base_lines: int | None,
    current_lines: int,
) -> str | None:
    if base_lines is None or base_lines <= limit:
        if current_lines > limit:
            return f"{label} exceeds {limit} non-empty lines: {path} ({current_lines})"
        return None
    if current_lines > base_lines:
        return (
            f"historical {label} exceeds its {base_lines}-line baseline and grew: "
            f"{path} ({current_lines})"
        )
    return None


def load_allowlist(path: Path) -> set[str]:
    if not path.exists():
        return set()
    return {
        line.split("#", 1)[0].strip()
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.split("#", 1)[0].strip()
    }


def validate_json(path: str, content: bytes) -> str | None:
    if not path.endswith(".json"):
        return None
    try:
        json.loads(content)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        return f"invalid JSON: {path}: {error}"
    return None


def validate_changes(
    source: ChangeSource,
    max_blob_bytes: int,
    allowlist: set[str],
) -> tuple[list[str], list[str], int]:
    errors: list[str] = []
    warnings: list[str] = []
    paths = changed_paths(source)
    for change in paths:
        path = change.path
        content = current_content(source, path)
        if content is None:
            continue
        if len(content) > max_blob_bytes and path not in allowlist:
            errors.append(
                f"blob exceeds {max_blob_bytes} bytes: {path} ({len(content)} bytes)"
            )
        if json_error := validate_json(path, content):
            errors.append(json_error)
        policy = line_limit(path)
        current_lines = count_non_empty_lines(content)
        if policy is None or current_lines is None:
            continue
        limit, label = policy
        previous = base_content(source, change.base_path)
        base_lines = count_non_empty_lines(previous) if previous is not None else None
        if error := line_limit_error(
            path,
            label,
            limit,
            base_lines,
            current_lines,
        ):
            errors.append(error)
        if (
            label == "Rust source"
            and current_lines > RUST_PRODUCTION_TARGET_LINES
            and (base_lines is None or base_lines <= RUST_PRODUCTION_TARGET_LINES)
        ):
            warnings.append(
                f"Rust source exceeds the {RUST_PRODUCTION_TARGET_LINES}-line target: "
                f"{path} ({current_lines})"
            )
    return errors, warnings, len(paths)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument("--cached", action="store_true")
    mode.add_argument("--base")
    parser.add_argument("--head")
    parser.add_argument(
        "--max-blob-bytes",
        type=int,
        default=DEFAULT_MAX_BLOB_BYTES,
    )
    parser.add_argument(
        "--allowlist",
        type=Path,
        default=Path(".github/blob-size-allowlist.txt"),
    )
    args = parser.parse_args()
    if args.base and not args.head:
        parser.error("--head is required with --base")
    return args


def main() -> int:
    args = parse_args()
    source = ChangeSource(
        cached=args.cached,
        base=args.base,
        head=args.head,
    )
    errors, warnings, checked = validate_changes(
        source,
        args.max_blob_bytes,
        load_allowlist(args.allowlist),
    )
    for warning in warnings:
        print(f"warning: {warning}", file=sys.stderr)
    if errors:
        print("changed-file policy failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1
    print(f"changed-file policy passed ({checked} file(s) checked)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
