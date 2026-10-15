import unittest

from pr_change_scope import is_document, scope_matches, should_run


class PullRequestChangeScopeTests(unittest.TestCase):
    def test_sync_contract_changes_run_the_rust_guard(self):
        for path in ["contracts/talos-sync/metrics-v1/dto.rs", "contracts/talos-sync/metrics-v1/schema.json", "scripts/sync-metrics-contract.py"]:
            self.assertTrue(scope_matches("rust", path))
        self.assertFalse(scope_matches("rust", "contracts/talos-sync/metrics-v1/README.md"))

    def test_document_files_do_not_trigger_product_jobs(self):
        for path in (
            "docs/development/DEVELOPMENT.md",
            "talos-rs/README.md",
            "talos-app/README.md",
            "talos-e2e/README.md",
            "conf/README.md",
            "spec/design.rst",
            "CONTRIBUTING_zh.md",
        ):
            with self.subTest(path=path):
                self.assertTrue(is_document(path))
                for scope in ("rust", "frontend", "pr-e2e", "tui-install"):
                    self.assertFalse(scope_matches(scope, path))

    def test_backend_directory(self):
        path = "talos-rs/crates/base/src/lib.rs"
        self.assertTrue(scope_matches("rust", path))
        self.assertTrue(scope_matches("pr-e2e", path))
        self.assertTrue(scope_matches("tui-install", path))
        self.assertFalse(scope_matches("frontend", path))

    def test_frontend_directory(self):
        path = "talos-app/src/main.tsx"
        self.assertTrue(scope_matches("frontend", path))
        self.assertTrue(scope_matches("pr-e2e", path))
        self.assertFalse(scope_matches("rust", path))
        self.assertFalse(scope_matches("tui-install", path))

    def test_configuration_directory(self):
        path = "conf/talos-prod.yaml"
        self.assertTrue(scope_matches("pr-e2e", path))
        self.assertTrue(scope_matches("tui-install", path))
        self.assertFalse(scope_matches("rust", path))
        self.assertFalse(scope_matches("frontend", path))

    def test_e2e_directory_and_installer_subdirectory(self):
        self.assertTrue(scope_matches("pr-e2e", "talos-e2e/scenarios/14_sandbox/01_case.sh"))
        self.assertFalse(scope_matches("tui-install", "talos-e2e/scenarios/14_sandbox/01_case.sh"))
        self.assertTrue(
            scope_matches("tui-install", "talos-e2e/scenarios/15_tui_install/01_native.sh")
        )

    def test_other_directories_do_not_trigger_product_jobs(self):
        paths = [
            "docs/guide.md",
            "design/overview.md",
            "spec/design.md",
            ".github/workflows/talos-rust.yml",
            ".github/ISSUE_TEMPLATE/agent-task.yml",
            "scripts/check_error_contract.py",
            "build/README.md",
        ]
        for scope in ("rust", "frontend", "pr-e2e", "tui-install"):
            with self.subTest(scope=scope):
                self.assertFalse(should_run(scope, paths))


if __name__ == "__main__":
    unittest.main()
