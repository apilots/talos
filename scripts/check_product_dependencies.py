#!/usr/bin/env python3
"""Check isolated product dependency graphs instead of workspace feature unions."""

import subprocess
from pathlib import Path


WORKSPACE = Path(__file__).resolve().parents[1] / "talos-rs"
SERVER_LIBRARIES = {"talos-control-plane", "talos-worker-runtime", "talos-relay-server"}


def dependencies(package: str, *options: str) -> set[str]:
    result = subprocess.run(
        ["cargo", "tree", "--locked", "--edges", "normal,build", "--prefix", "none",
         "--format", "{p}", "-p", package, *options],
        cwd=WORKSPACE, check=False, text=True, capture_output=True,
    )
    if result.returncode:
        raise SystemExit(f"Cannot inspect {package} dependencies:\n{result.stderr}")
    return {line.split()[0] for line in result.stdout.splitlines() if line.strip()}


def main() -> None:
    for package in ("talos-base", "talos-sdk-embedded", "talos-tui", "app-server", "talos-worker-bin"):
        for options in ((), ("--no-default-features",)):
            graph = dependencies(package, *options)
            unexpected = graph & SERVER_LIBRARIES
            if unexpected:
                raise SystemExit(f"{package} forces Server libraries: {', '.join(sorted(unexpected))}")
            if options and "talos-sync" in graph:
                raise SystemExit(f"{package} forces Sync without product features")
            print(f"PASS {package} {options}: excludes Server libraries; optional Sync stays optional")

    if dependencies("talos-server") & SERVER_LIBRARIES != SERVER_LIBRARIES:
        raise SystemExit("Talos Server must explicitly assemble control plane, managed runtime and Relay")
    minimal = dependencies("talos-server", "--no-default-features")
    if minimal & SERVER_LIBRARIES != {"talos-control-plane", "talos-worker-runtime"} or "talos-sync" in minimal:
        raise SystemExit("Minimal Talos Server must retain authority without forcing Relay or Sync")
    print("PASS talos-server: full and minimal cloud compositions are explicit")

    for features in ("server", "server,sync", "server,relay", "server,relay,sync"):
        graph = dependencies("talos-base", "--no-default-features", "--features", features)
        required = {"talos-control-plane", "talos-worker-runtime"}
        if "relay" in features.split(","):
            required.add("talos-relay-server")
        actual = graph & SERVER_LIBRARIES
        if actual != required:
            raise SystemExit(f"Server {features}: expected {sorted(required)}, found {sorted(actual)}")
        print(f"PASS Server {features}: explicit authority and Relay composition")


if __name__ == "__main__":
    main()
