"""Exercise the real runner in a disposable checkout, without product E2E calls."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

E2E = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("e2e_runner", E2E / "scripts/runner.py")
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "checkout/e2e"
        (self.root / "scripts").mkdir(parents=True)
        self.scenarios = self.root / "scenarios"
        self.scenarios.mkdir()
        shutil.copy2(E2E / "run_e2e.sh", self.root / "run_e2e.sh")
        shutil.copy2(E2E / "scripts/runner.py", self.root / "scripts/runner.py")
        (self.root / "scripts/common.sh").write_text('''
export TALOS_E2E_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="${TALOS_E2E_DIR}/.."
REPORT_ROOT="${TALOS_E2E_DIR}/reports"
TALOS_RS_DIR="${TALOS_E2E_DIR}"
TALOS_E2E_TMP_ROOT="${TALOS_E2E_DIR}/tmp"
E2E_TMP_ROOT="${TALOS_E2E_TMP_ROOT}"
load_env() { touch "${TALOS_E2E_DIR}/env-loaded"; }
build_server_binary() { echo server >> "${TALOS_E2E_DIR}/built"; }
build_cli_binary() { echo cli >> "${TALOS_E2E_DIR}/built"; }
default_talos_home() { printf '%s/homes/%s' "${E2E_TMP_ROOT}" "${TALOS_SCENARIO_NAME}"; }
has_real_llm_key() { return 1; }
''')
        self.bin = self.root / "bin"
        self.bin.mkdir()
        (self.bin / "cargo").write_text('#!/bin/sh\ntouch "$(dirname "$0")/../built"\n')
        (self.bin / "cargo").chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"])
        self.add_case("01_root.sh")
        self.add_case("18_existing/one.sh")
        self.add_case("18_existing/two.sh")

    def add_case(self, name, exit_code=0):
        path = self.scenarios / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('#!/bin/bash\nprintf "%s:%s\\n" "${TALOS_SCENARIO_NAME}" '
                        '"${TALOS_E2E_GROUP_NAME}" >> "${TALOS_E2E_DIR}/executed"\n'
                        f'echo "CASE {path.stem} PASS"\nexit {exit_code}\n')
        self.declare(path.parent, {})
        return path

    def declare(self, folder, default, cases=None):
        (folder / "requirements.json").write_text(json.dumps({"default": default, "cases": cases or {}}))

    def call(self, *args):
        return subprocess.run(["bash", str(self.root / "run_e2e.sh"), *args],
                              env=self.env, text=True, capture_output=True, timeout=15)

    def assert_no_execution(self):
        self.assertFalse((self.root / "built").exists())
        self.assertFalse((self.root / "executed").exists())

    def test_new_numbered_group_and_root_case_are_discovered_without_runner_edit(self):
        self.add_case("25_new/new.sh")
        self.add_case("26_new_root.sh")
        helper = self.scenarios / "helpers/ignored.sh"
        helper.parent.mkdir()
        helper.write_text("exit 99")
        result = self.call("--list")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.splitlines(), ["01_root.sh", "18_existing/one.sh",
            "18_existing/two.sh", "25_new/new.sh", "26_new_root.sh"])
        self.assert_no_execution()
        self.assertFalse((self.root / "env-loaded").exists())
        self.assertFalse((self.root / "reports").exists())
        self.assertFalse((self.root / "tmp").exists())

    def test_case_and_group_are_filters_of_the_same_catalog(self):
        all_cases = self.call("--list").stdout.splitlines()
        group = self.call("--group", "18_existing", "--list")
        self.assertEqual(group.stdout.splitlines(), [p for p in all_cases if p.startswith("18_existing/")])
        for case in ("one", "18_existing/one", "18_existing/one.sh"):
            result = self.call("--case", case, "--list")
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertEqual(result.stdout, "18_existing/one.sh\n")

    def test_invalid_selectors_never_build_or_run(self):
        for args in [("--case",), ("--group", "--list"), ("--case", "one", "--group", "18_existing"),
                     ("--case", "one", "--case", "two"), ("--list", "--check"), ("--unknown",),
                     ("--group", "missing", "--list"), ("--case", "../run_e2e", "--list")]:
            with self.subTest(args=args):
                self.assertNotEqual(self.call(*args).returncode, 0)
        self.assert_no_execution()
        self.assertFalse((self.root / "reports").exists())

    def test_ambiguous_basename_requires_relative_path(self):
        self.add_case("25_new/one.sh")
        result = self.call("--case", "one", "--list")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Ambiguous", result.stderr)
        self.assertIn("25_new/one.sh", result.stderr)
        self.assertEqual(self.call("--case", "18_existing/one", "--list").returncode, 0)

    def test_exact_root_path_wins_over_a_duplicate_basename(self):
        self.add_case("25_new/01_root.sh")
        result = self.call("--case", "01_root.sh", "--list")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "01_root.sh\n")

    def test_symlink_scenario_cannot_escape_catalog(self):
        target = self.root / "outside.sh"
        target.write_text("exit 99")
        (self.scenarios / "27_escape.sh").symlink_to(target)
        self.assertNotEqual(self.call("--list").returncode, 0)
        self.assert_no_execution()

    def test_missing_dependencies_are_aggregated_before_build_or_execution(self):
        self.declare(self.scenarios / "18_existing", {"commands": ["missing-command-A", "missing-command-B"],
                     "env": ["E2E_RUNNER_MISSING_KEY"]})
        self.env.pop("E2E_RUNNER_MISSING_KEY", None)
        result = self.call("--group", "18_existing")
        self.assertNotEqual(result.returncode, 0)
        for text in ["missing-command-A", "missing-command-B", "E2E_RUNNER_MISSING_KEY", "one.sh", "two.sh", "blocked before build"]:
            self.assertIn(text, result.stderr)
        self.assert_no_execution()
        self.assertEqual(len(list((self.root / "reports").glob("preflight_*.log"))), 1)

    def test_check_only_and_scoped_exception_do_not_require_unselected_dependencies(self):
        self.declare(self.scenarios / "18_existing", {"commands": ["missing-command"]},
                     {"one.sh": {"commands": []}})
        result = self.call("--case", "one", "--check")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Preflight PASS: 1", result.stdout)
        self.assert_no_execution()
        self.assertNotEqual(self.call("--group", "18_existing", "--check").returncode, 0)

    def test_unavailable_docker_never_hides_selected_cases(self):
        docker = self.bin / "docker"
        docker.write_text("#!/bin/sh\nexit 1\n")
        docker.chmod(0o755)
        self.declare(self.scenarios / "18_existing", {"checks": ["docker"]})
        self.assertEqual(len(self.call("--group", "18_existing", "--list").stdout.splitlines()), 2)
        result = self.call("--group", "18_existing", "--check")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Docker daemon", result.stderr)
        self.assert_no_execution()

    def test_unsupported_platform_blocks_scope_instead_of_skipping(self):
        self.declare(self.scenarios, {"platforms": ["unsupported-test-platform"]})
        result = self.call("--check")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("requires platform unsupported-test-platform", result.stderr)
        self.assert_no_execution()

    def test_prebuild_is_driven_by_selected_case_declaration(self):
        self.declare(self.scenarios / "18_existing", {"binaries": ["cli"]},
                     {"one.sh": {"binaries": ["server"]}})
        result = self.call("--case", "one")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("server", (self.root / "built").read_text())
        self.assertNotIn("cli", (self.root / "built").read_text())

    def test_missing_or_stale_dependency_declarations_are_explicit_errors(self):
        folder = self.add_case("25_new/new.sh").parent
        (folder / "requirements.json").unlink()
        self.assertIn("Missing dependency declaration", self.call("--group", "25_new", "--check").stderr)
        self.declare(folder, {}, {"gone.sh": {}})
        self.assertIn("Unknown case override", self.call("--group", "25_new", "--check").stderr)
        self.declare(folder, {"checks": ["unknown-check"]})
        self.assertIn("Unknown dependency", self.call("--group", "25_new", "--check").stderr)

    def test_run_uses_selected_scope_and_keeps_failure_exit_and_report(self):
        self.add_case("25_new/fail.sh", 7)
        self.add_case("25_new/last.sh")
        result = self.call()
        self.assertEqual(result.returncode, 1)
        executed = (self.root / "executed").read_text().splitlines()
        self.assertIn("18_existing/one:18_existing", executed)
        self.assertIn("25_new/last:25_new", executed)
        report = (self.root / "reports/latest_report.md").read_text()
        self.assertIn("25_new/fail.sh", report)
        self.assertIn("FAIL", report)
        self.assertIn("- 失败: 1", report)
        self.assertEqual(len(executed), 5)

    def test_single_case_runs_exactly_once(self):
        result = self.call("--case", "one")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.root / "executed").read_text(), "18_existing/one:\n")

    def test_preflight_deduplicates_probes_and_preserves_secrets(self):
        self.declare(self.scenarios / "18_existing", {"checks": ["docker"], "env": ["E2E_TEST_SECRET"]})
        output = io.StringIO()
        with patch.dict(os.environ, {"E2E_TEST_SECRET": "never-print-this-value"}), patch.object(runner, "probe", return_value=False) as probe, contextlib.redirect_stderr(output):
            self.assertEqual(runner.preflight(self.scenarios, ["18_existing/one.sh", "18_existing/two.sh"]), 1)
        self.assertEqual(probe.call_count, 1)
        self.assertIn("Docker daemon", output.getvalue())
        self.assertNotIn("never-print-this-value", output.getvalue())

    def test_container_cases_do_not_require_native_host_packaging_dependencies(self):
        # A clean Linux runner has Docker/Cargo but no native packaging protoc/PyYAML.
        with patch.object(runner.platform, "system", return_value="Linux"), \
             patch.object(runner, "executable", side_effect=lambda name: name in {"docker", "cargo"}), \
             patch.object(runner, "probe", side_effect=lambda argv: argv == ["docker", "info"]), \
             contextlib.redirect_stdout(io.StringIO()):
            for case in ["14_sandbox/99_linux_docker.sh", "15_tui_install/02_linux_docker.sh"]:
                with self.subTest(case=case):
                    self.assertEqual(runner.preflight(E2E / "scenarios", [case]), 0)

    def test_native_installer_checks_build_tools_without_unneeded_yaml_or_bubblewrap(self):
        with patch.object(runner.platform, "system", return_value="Linux"), \
             patch.object(runner, "executable", side_effect=lambda name: name in {"cargo", "protoc", "curl", "tar", "python3"}), \
             patch.object(runner, "probe", return_value=False), \
             contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(runner.preflight(E2E / "scenarios", ["15_tui_install/01_native_install.sh"]), 0)

    def test_real_catalog_has_valid_dependencies_and_build_ownership(self):
        catalog = runner.discover(E2E / "scenarios")
        self.assertTrue(catalog)
        for case in catalog:
            with self.subTest(case=case):
                runner.requirements(E2E / "scenarios", case)
        command = [sys.executable, str(E2E / "scripts/runner.py"), "binaries", str(E2E / "scenarios")]
        smoke = subprocess.run(command + ["09_app_server/smoke_e2e.sh"], capture_output=True, text=True, check=True)
        self.assertEqual(smoke.stdout.strip(), "")
        cli = subprocess.run(command + ["06_tools/cli_bash.sh"], capture_output=True, text=True, check=True)
        self.assertEqual(cli.stdout.strip(), "cli")


if __name__ == "__main__":
    unittest.main()
