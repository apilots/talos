import unittest

from pr_change_scope import is_document_only_path, scope_matches, should_run


class PullRequestChangeScopeTests(unittest.TestCase):
    def test_document_paths_are_not_product_inputs(self):
        for path in (
            "docs/development/DEVELOPMENT.md",
            "talos-rs/README.md",
            "specs/design.rst",
            ".github/ISSUE_TEMPLATE/agent-task.yml",
            ".github/PULL_REQUEST_TEMPLATE.md",
            "CONTRIBUTING_zh.md",
        ):
            with self.subTest(path=path):
                self.assertTrue(is_document_only_path(path))

    def test_product_configuration_runs_applicable_checks(self):
        self.assertTrue(scope_matches("pr-e2e", "conf/talos-prod.yaml"))
        self.assertTrue(scope_matches("tui-install", "conf/talos-prod.yaml"))
        self.assertFalse(scope_matches("frontend", "conf/talos-prod.yaml"))

    def test_product_source_runs_only_relevant_scopes(self):
        self.assertTrue(scope_matches("rust", "talos-rs/crates/base/src/lib.rs"))
        self.assertTrue(scope_matches("pr-e2e", "talos-rs/crates/base/src/lib.rs"))
        self.assertFalse(scope_matches("frontend", "talos-rs/crates/base/src/lib.rs"))
        self.assertTrue(scope_matches("frontend", "talos-app/src/main.tsx"))
        self.assertFalse(scope_matches("rust", "talos-app/src/main.tsx"))

    def test_document_only_changes_skip_resource_heavy_jobs(self):
        paths = ["docs/guide.md", "CONTRIBUTING.md"]
        for scope in ("pr-e2e", "rust", "frontend", "tui-install"):
            with self.subTest(scope=scope):
                self.assertFalse(should_run(scope, paths))

    def test_unrelated_metadata_does_not_run_product_jobs(self):
        paths = [
            ".github/ISSUE_TEMPLATE/agent-task.yml",
            ".github/labeler.yml",
            ".github/workflows/talos-rust.yml",
            ".github/workflows/talos-frontend-app.yml",
            ".github/workflows/talos-tui-install.yml",
            ".github/workflows/pr-e2e.yml",
        ]
        for scope in ("pr-e2e", "rust", "frontend", "tui-install"):
            with self.subTest(scope=scope):
                self.assertFalse(should_run(scope, paths))


if __name__ == "__main__":
    unittest.main()
