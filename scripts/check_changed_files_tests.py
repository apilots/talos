import unittest

from check_changed_files import (
    frontend_line_limit,
    line_limit_error,
    parse_name_status,
    rust_line_limit,
    validate_json,
)


class ChangedFilePolicyTests(unittest.TestCase):
    def test_frontend_limits_match_file_roles(self):
        self.assertEqual(
            frontend_line_limit("talos-app/web/src/features/chat/components/Card.tsx"),
            (200, "frontend component"),
        )
        self.assertEqual(
            frontend_line_limit("talos-app/web/src/pages/Home.tsx"),
            (300, "frontend page"),
        )
        self.assertEqual(
            frontend_line_limit("talos-app/web/src/pages/Home.test.tsx"),
            (300, "frontend test"),
        )

    def test_rust_limits_separate_source_and_tests(self):
        self.assertEqual(
            rust_line_limit("talos-rs/crates/example/src/lib.rs"),
            (800, "Rust source"),
        )
        self.assertEqual(
            rust_line_limit("talos-rs/crates/example/src/lib_tests.rs"),
            (1200, "Rust test"),
        )

    def test_new_file_must_respect_limit(self):
        self.assertIsNotNone(
            line_limit_error("new.rs", "Rust source", 800, None, 801)
        )

    def test_historical_over_limit_file_may_not_grow(self):
        self.assertIsNone(
            line_limit_error("old.rs", "Rust source", 800, 1000, 999)
        )
        self.assertIsNotNone(
            line_limit_error("old.rs", "Rust source", 800, 1000, 1001)
        )

    def test_invalid_json_is_rejected(self):
        self.assertIsNotNone(validate_json("config.json", b"{"))
        self.assertIsNone(validate_json("config.json", b'{"ok": true}'))

    def test_rename_keeps_the_original_baseline_path(self):
        changes = parse_name_status(b"R100\0old.ts\0new.ts\0M\0same.rs\0A\0new.rs\0")

        self.assertEqual(
            [(change.path, change.base_path) for change in changes],
            [("new.rs", None), ("new.ts", "old.ts"), ("same.rs", "same.rs")],
        )


if __name__ == "__main__":
    unittest.main()
