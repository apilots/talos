"""Run the FrontierHarness launcher inside a prepared Runta checkpoint."""

import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys
import tarfile
import tempfile
import time
from uuid import uuid4


ROOT = "/work/talos-pilot"
JOB = "/work/talos-bench-run"
LAUNCH = r"""
set -eu
export PATH="/root/.local/bin:$PATH"
export OPENAI_API_KEY=runta-secret-stub
export TALOS_BENCH_PLATFORM=linux/amd64
export TALOS_BENCH_PRODUCT=/work/talos-pilot/install/product
export TALOS_BENCH_EXTRA_DOCKER_COMPOSE=/work/talos-pilot/runta-ca.yaml
code=0
/work/talos-pilot/source/talos-bench/run.sh frontier-harness@1.1.0 --backend local \
    > /work/talos-bench-run/launcher.log 2>&1 || code=$?
printf '%s\n' "$code" > /work/talos-bench-run/exit-code
exit "$code"
"""
ARCHIVE = r"""
set -eu
cd /work/talos-bench-run
job=$(sed -n 's/^Talos benchmark run: //p' launcher.log)
files=(launcher.log exit-code)
if [[ -n "$job" ]]; then
    test "$(dirname "$job")" = /work/talos-pilot/source/output/talos-bench
    files+=(-C "$job" .)
fi
tar -czf job.tar.gz "${files[@]}"
sha256sum job.tar.gz
"""


def invoke(*args, timeout=60):
    executable = os.environ.get("TALOS_BENCH_RUNTA_CLI", "runta")
    result = subprocess.run([executable, *args], check=False, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f"Runta {args[0]} failed: {(result.stderr or result.stdout).strip()}")
    return json.loads(result.stdout)


def remote(runtime, command):
    result = invoke("exec", runtime, "--", "bash", "-c", command)
    if result["status"] != 0:
        raise RuntimeError(f"Runta remote command failed: {result['stderr'].strip()}")
    return result["stdout"].strip()


def upload_launcher(repo, runtime):
    tracked = subprocess.check_output(
        ["git", "-C", repo, "ls-files", "-z", "--", "talos-bench"],
    ).decode().split("\0")
    with tempfile.TemporaryDirectory() as temp:
        archive = Path(temp) / "bench.tar.gz"
        with tarfile.open(archive, "w:gz") as package:
            for name in filter(None, tracked):
                package.add(repo / name, arcname=name, recursive=False)
        invoke("cp", str(archive), f"{runtime}:{JOB}/bench.tar.gz")
    remote(runtime, f"tar -xzf {JOB}/bench.tar.gz -C {ROOT}/source")


def release(runtime):
    invoke("shutdown", runtime, "--wait", "--timeout-secs", "240", timeout=250)
    invoke("rm", runtime, "--wait", "--timeout", "120", timeout=130)


def run(repo, tasks, repeats):
    checkpoint = os.environ.get("TALOS_BENCH_RUNTA_CHECKPOINT")
    secret = os.environ.get("TALOS_BENCH_RUNTA_SECRET")
    if not checkpoint or not secret:
        raise ValueError("Set TALOS_BENCH_RUNTA_CHECKPOINT and TALOS_BENCH_RUNTA_SECRET.")
    environment = {
        "HOME": "/root",
        "TALOS_BENCH_MODEL": os.environ["TALOS_BENCH_MODEL"],
        "TALOS_BENCH_REASONING_EFFORT": os.environ["TALOS_BENCH_REASONING_EFFORT"],
        "TALOS_BENCH_TASKS": tasks,
        "TALOS_BENCH_REPEATS": repeats,
        "TALOS_BENCH_AGENT_ENV": os.environ.get("TALOS_BENCH_AGENT_ENV", ""),
    }
    runtime = f"talos-frontier-{uuid4().hex[:12]}"
    directory = repo / "output/talos-bench"
    directory.mkdir(parents=True, exist_ok=True)
    output = Path(tempfile.mkdtemp(prefix="run.", dir=directory))
    print(f"Talos benchmark run: {output}\nRunta runtime: {runtime}", flush=True)
    invoke("checkpoint", "restore", checkpoint, runtime, "--wait", "--timeout-secs", "300", timeout=320)
    try:
        remote(runtime, f"""set -eu
test "$(uname -m)" = x86_64
test "$(docker version --format '{{{{.Server.Os}}}}/{{{{.Server.Arch}}}}')" = linux/amd64
test "$(git -C {ROOT}/source rev-parse HEAD)" = "$(cat {ROOT}/install/product-revision.txt)"
test -r {ROOT}/runta-ca.yaml
mkdir {JOB}
""")
        upload_launcher(repo, runtime)
        invoke("secret", "rule", "set", runtime, "--secret", secret, "--host", "api.openai.com",
               "--path", "/v1/*", "--header", "Authorization", "--template", "Bearer ${secret}")
    except (Exception, KeyboardInterrupt):
        try:
            release(runtime)
        except Exception as error:
            print(f"Could not release Runta runtime {runtime}: {error}", file=sys.stderr)
        raise
    unit = "talos-frontier-bench"
    remote(runtime, shlex.join(["systemd-run", f"--unit={unit}",
                                *(f"--setenv={key}={value}" for key, value in environment.items()),
                                "bash", "-c", LAUNCH]))
    while True:
        code = remote(runtime, f"""if test -f {JOB}/exit-code; then
    cat {JOB}/exit-code
elif ! systemctl is-active --quiet {unit}; then
    exit 1
fi""")
        if code:
            break
        time.sleep(15)
    digest = remote(runtime, ARCHIVE).split()[0]
    archive = output / "original-job.tar.gz"
    for attempt in range(2):
        try:
            invoke("cp", f"{runtime}:{JOB}/job.tar.gz", str(archive), timeout=120)
            break
        except (RuntimeError, subprocess.TimeoutExpired):
            if attempt:
                raise
    with archive.open("rb") as handle:
        if hashlib.file_digest(handle, "sha256").hexdigest() != digest:
            raise RuntimeError("Runta archive checksum does not match")
    with tarfile.open(archive) as package:
        package.extractall(output, filter="data")
    release(runtime)
    return int(code)


if __name__ == "__main__":
    repo_path, selected, attempts = sys.argv[1:]
    try:
        sys.exit(run(Path(repo_path), selected, attempts))
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
