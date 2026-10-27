"""Filesystem scenario selection and read-only prerequisites for the shell runner."""
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys

NUMBERED = re.compile(r"^[0-9]{2}_.+")
FIELDS = {"commands", "python_modules", "checks", "platforms", "env", "binaries"}
CHECKS = {"docker", "cloud", "model", "chrome", "display", "caddy", "worker_image", "server_image", "sandbox", "standalone_docker"}


def discover(root):
    cases = []
    for entry in root.iterdir():
        if not NUMBERED.fullmatch(entry.name):
            continue
        candidates = entry.glob("*.sh") if entry.is_dir() else [entry]
        for path in candidates:
            if path.suffix != ".sh" or not path.is_file():
                continue
            if entry.is_symlink() or path.is_symlink():
                raise ValueError(f"Scenario symlinks are not allowed: {path}")
            cases.append(path.relative_to(root).as_posix())
    return sorted(cases)


def select(root, group="", case=""):
    catalog = discover(root)
    if group:
        selected = [name for name in catalog if Path(name).parent.as_posix() == group]
    elif case:
        name = case.removesuffix(".sh")
        selected = [path for path in catalog if path.removesuffix(".sh") == name]
        if not selected and "/" not in name:
            selected = [path for path in catalog if Path(path).stem == name]
        if len(selected) > 1:
            raise ValueError(f"Ambiguous scenario case: {case}\n" + "\n".join(selected))
    else:
        selected = catalog
    if not selected:
        raise ValueError(f"No scenarios found for {group or case or root}")
    return selected


def requirements(root, case):
    folder = (root / case).parent
    path = folder / "requirements.json"
    if not path.is_file():
        raise ValueError(f"Missing dependency declaration: {path}")
    document = json.loads(path.read_text())
    if not isinstance(document, dict) or set(document) - {"default", "cases"}:
        raise ValueError(f"Unknown declaration fields: {path}")
    overrides = document.get("cases", {})
    if not isinstance(document.get("default", {}), dict) or not isinstance(overrides, dict):
        raise ValueError(f"Invalid dependency declaration: {path}")
    for name in overrides:
        if Path(name).name != name or not (folder / name).is_file():
            raise ValueError(f"Unknown case override {name}: {path}")
    result = {**document.get("default", {}), **overrides.get(Path(case).name, {})}
    if set(result) - FIELDS or any(not isinstance(v, list) or
            any(not isinstance(item, str) for item in v) for v in result.values()):
        raise ValueError(f"Invalid dependency declaration: {path}")
    if set(result.get("checks", [])) - CHECKS or set(result.get("binaries", [])) - {"server", "cli", "worker", "app_server", "test_client", "sandbox"}:
        raise ValueError(f"Unknown dependency or binary: {path}")
    return result


def probe(argv):
    try:
        return subprocess.run(argv, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                              timeout=15, check=False).returncode == 0
    except (OSError, subprocess.TimeoutExpired):
        return False


def executable(name):
    return bool(shutil.which(name))


def check_dependency(kind, value, root):
    if kind == "commands":
        return None if executable(value) else f"install executable {value}"
    if kind == "python_modules":
        return None if probe([sys.executable, "-c", "import " + value]) else f"install Python module {value} for {sys.executable}"
    if kind == "env":
        return None if os.environ.get(value) else f"set {value} (value is never printed)"
    if kind == "platforms":
        return None if platform.system() in value.split(",") else f"requires platform {value}; current {platform.system()}"
    if value == "docker":
        return None if probe(["docker", "info"]) else "start an accessible Docker daemon"
    if value == "cloud":
        binary = os.environ.get("TALOS_CLOUD_SERVER_BIN", "cloud-server")
        return None if executable(binary) else "set TALOS_CLOUD_SERVER_BIN to an executable Cloud fixture"
    if value == "model":
        common = root.parent / "scripts/common.sh"
        return None if probe(["bash", "-c", 'source "$1"; has_real_llm_key', "preflight", str(common)]) else "configure a real provider key in .env (existing has_real_llm_key contract)"
    if value == "chrome":
        binary = os.environ.get("TALOS_E2E_CHROME_BIN", "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")
        return None if executable(binary) else "set TALOS_E2E_CHROME_BIN to installed Chrome"
    if value == "caddy":
        binary = os.environ.get("TALOS_E2E_CADDY_BIN", "caddy")
        return None if executable(binary) else "install Caddy or set TALOS_E2E_CADDY_BIN"
    if value == "display":
        return None if platform.system() != "Linux" or os.environ.get("DISPLAY") else "run native Desktop under xvfb-run -a or provide DISPLAY"
    if value == "worker_image":
        image = os.environ.get("TALOS_E2E_WORKER_IMAGE", "talos-worker:browser-e2e")
        return None if probe(["docker", "image", "inspect", image]) else "build the current Worker image; see group20 README / TALOS_E2E_WORKER_IMAGE"
    if value == "server_image":
        image = os.environ.get("TALOS_E2E_SERVER_IMAGE", "talos-server:browser-e2e")
        return None if probe(["docker", "image", "inspect", image]) else "build the current Server image; see group20 README / TALOS_E2E_SERVER_IMAGE"
    if value == "sandbox":
        if platform.system() == "Linux":
            return None if executable("bwrap") else "install bubblewrap for the native Linux sandbox"
    if value == "standalone_docker" and platform.system() == "Darwin" and os.environ.get("TALOS_E2E_SINGLE_CASE"):
        return None if probe(["docker", "info"]) else "standalone macOS cases also require Docker Linux acceptance"
    return None


def preflight(root, cases):
    dependencies = {("commands", "ps"): set(cases)}
    if platform.system() == "Darwin":
        dependencies[("commands", "lsof")] = set(cases)
    for case in cases:
        req = requirements(root, case)
        for kind, values in req.items():
            if kind == "binaries":
                continue
            entries = [",".join(values)] if kind == "platforms" else values
            for value in entries:
                dependencies.setdefault((kind, value), set()).add(case)
    failures = []
    for (kind, value), affected in sorted(dependencies.items()):
        problem = check_dependency(kind, value, root)
        if problem:
            failures.append(f"BLOCKED {kind}:{value}: {problem}\n  cases: {', '.join(sorted(affected))}")
    if failures:
        print("\n".join(failures), file=sys.stderr)
        return 1
    print(f"Preflight PASS: {len(cases)} selected scenarios; no model calls or services started")
    return 0


def main(argv):
    if sys.version_info < (3, 9):
        raise ValueError("Python 3.9+ is required")
    action, root_arg, *args = argv
    root = Path(root_arg).resolve()
    if action == "select":
        print("\n".join(select(root, *args)))
        return 0
    catalog = discover(root)
    if not args or any(case not in catalog for case in args):
        raise ValueError("Preflight/build selection must come from the discovered catalog")
    if action == "check":
        return preflight(root, args)
    if action == "binaries":
        print("\n".join(sorted({binary for case in args for binary in requirements(root, case).get("binaries", [])})))
        return 0
    raise ValueError(f"Unknown runner action: {action}")


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except (ValueError, OSError, TypeError) as error:
        print(f"Runner error: {error}", file=sys.stderr)
        sys.exit(2)
