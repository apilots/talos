import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("e2e_report", Path(__file__).resolve().parents[1] / "scripts/report_failures.py")
report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(report)


class FailureReportTests(unittest.TestCase):
    def test_real_report_status_selects_only_failed_cases_and_bounds_output(self):
        with tempfile.TemporaryDirectory() as home:
            root = Path(home)
            (root / "e2e_report_1.md").write_text(
                '### 场景 good.sh\n- 状态: PASS\n```text\nignore-good\n```\n'
                '### 场景 bad.sh\n- 状态: FAIL\n```text\n'
                + '\n'.join(f'line-{n}' for n in range(100)) + '\n```\n')
            rows = list(report.failures(root, {}))
        self.assertEqual(len(rows), 1)
        self.assertIn("Failed scenario: bad.sh", rows[0])
        self.assertNotIn("ignore-good", rows[0])
        self.assertNotIn("line-19\n", rows[0])
        self.assertIn("line-99", rows[0])

    def test_env_bearer_jwt_and_named_credentials_are_redacted(self):
        text = 'env-secret-value Bearer another-secret eyJheader.payload.signature {"api_key":"different-secret","access_token":"also-secret"}'
        actual = report.redact(text, {"TEST_API_KEY": "env-secret-value"})
        for secret in ["env-secret-value", "another-secret", "eyJheader.payload.signature", "different-secret", "also-secret"]:
            self.assertNotIn(secret, actual)
        self.assertIn("<redacted>", actual)

    def test_preflight_block_is_reported_even_without_a_case_report(self):
        with tempfile.TemporaryDirectory() as home:
            root = Path(home)
            (root / "preflight_1.log").write_text('BLOCKED commands:protoc: install protoc\n')
            (root / "preflight_2.log").write_text('Preflight PASS: 1 selected scenarios\n')
            rows = list(report.failures(root, {}))
        self.assertEqual(len(rows), 1)
        self.assertIn("Prerequisite failure: preflight_1.log", rows[0])
        self.assertIn("install protoc", rows[0])


if __name__ == "__main__":
    unittest.main()
