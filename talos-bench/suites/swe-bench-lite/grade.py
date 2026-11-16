"""Grade the submitted patch in a fresh verifier using official SWE-bench tests."""

from importlib.metadata import version
import json
from pathlib import Path
import platform
import subprocess
from tempfile import NamedTemporaryFile

from swebench.harness.constants import APPLY_PATCH_FAIL
from swebench.harness.grading import get_eval_report
from swebench.harness.utils import make_test_spec

TESTS = Path("/tests")
LOGS = Path("/logs/verifier")
SUBMISSION = Path("/logs/artifacts/submission.patch")
REPO = Path("/app")


def main():
    architecture = platform.machine()
    if architecture not in {"aarch64", "x86_64"}:
        raise RuntimeError(f"Unsupported SWE-bench Lite verifier architecture: {architecture}")
    (LOGS / "architecture.txt").write_text(f"verifier={architecture}\n")
    (LOGS / "swe-bench-version.txt").write_text(version("swebench") + "\n")
    case = json.loads((TESTS / "case.json").read_text())
    spec = make_test_spec(case)
    head = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip()
    if head != case["base_commit"]:
        raise RuntimeError("The verifier checkout is not at the benchmark base commit.")

    if not SUBMISSION.is_file():
        raise FileNotFoundError(SUBMISSION)
    patch = SUBMISSION.read_text(errors="surrogateescape")
    prediction = {"instance_id": spec.instance_id, "model_patch": patch or None}
    test_log = LOGS / "swe-bench-test-output.txt"
    applied = False
    with test_log.open("w") as log:
        if patch:
            result = subprocess.run(
                ["git", "apply", "--binary", str(SUBMISSION)], cwd=REPO,
                stdout=log, stderr=subprocess.STDOUT, check=False,
            )
            applied = result.returncode == 0
            if applied:
                with NamedTemporaryFile(mode="w", suffix=".sh") as script:
                    script.write(spec.eval_script)
                    script.flush()
                    subprocess.run(["bash", script.name], cwd=REPO,
                                   stdout=log, stderr=subprocess.STDOUT, check=True)
            else:
                log.write(APPLY_PATCH_FAIL + "\n")

    report = get_eval_report(spec, prediction, test_log, include_tests_status=True)
    outcome = report[spec.instance_id]
    (LOGS / "swe-bench-report.json").write_text(json.dumps(report, indent=2) + "\n")
    if applied and (not outcome["patch_successfully_applied"] or outcome["infra_failure"]):
        raise RuntimeError(f"Official SWE-bench tests did not finish. See {test_log}.")
    (LOGS / "reward.txt").write_text(f"{int(outcome['resolved'])}\n")


if __name__ == "__main__":
    main()
