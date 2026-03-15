#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

python3 - "${ROOT_DIR}" <<'PY'
import glob
import os
import re
import sys

root = sys.argv[1]
os.chdir(root)

files = glob.glob("crates/*/*/Cargo.toml") + glob.glob("apps/*/Cargo.toml") + glob.glob("bin/*/Cargo.toml")

order = {
    "infra": 0,
    "perception": 1,
    "action": 2,
    "cognition": 3,
    "orchestration": 4,
    "gateway": 5,
    "apps": 6,
    "bin": 7,
}

def layer(path: str) -> str:
    parts = path.split("/")
    if parts[0] == "crates":
        return parts[1]
    return parts[0]

def crate_name(text: str, fallback: str) -> str:
    m = re.search(r'^name\s*=\s*"([^"]+)"', text, re.M)
    return m.group(1) if m else fallback

violations = []

for file in sorted(files):
    with open(file, "r", encoding="utf-8") as fh:
        text = fh.read()
    src_layer = layer(file)
    src_name = crate_name(text, file)

    # meta/app/bin/common are not constrained by this guard
    if src_layer in {"meta", "apps", "bin", "common"}:
        continue

    for dep_name, dep_path in re.findall(r'^([A-Za-z0-9_\-]+)\s*=\s*\{[^\n]*path\s*=\s*"([^"]+)"[^\n]*\}', text, re.M):
        dep_toml = os.path.normpath(os.path.join(os.path.dirname(file), dep_path, "Cargo.toml"))
        dep_rel = os.path.relpath(dep_toml, ".")
        dst_layer = layer(dep_rel)

        if dst_layer in {"common", "meta", "bin"}:
            continue
        if src_layer not in order or dst_layer not in order:
            continue
        if order[src_layer] < order[dst_layer]:
            violations.append((src_name, src_layer, dep_name, dep_rel, dst_layer))

if violations:
    print("Layering violations detected:")
    for src_name, src_layer, dep_name, dep_rel, dst_layer in violations:
        print(f"- {src_name} ({src_layer}) -> {dep_name} [{dep_rel}] ({dst_layer})")
    sys.exit(1)

print("Layering guard passed.")
PY
