import unittest
from check_error_contract import violations


class ErrorContractTests(unittest.TestCase):
    def test_rejects_prose_based_error_decisions(self):
        for source in [
            'if error.message == "resource not found" {}',
            'if error.message !== `Expected English sentence.` {}',
            'if localization.english_message() != wire.message {}',
            'if error.message.contains("not found") {}',
            'if errorMessage(error).toLowerCase().includes("not found") {}',
        ]:
            with self.subTest(source=source):
                self.assertTrue(violations("sdk/adapters.ts", source))

    def test_rejects_duplicate_contract_and_public_diagnostic(self):
        for source in ["struct AgentErrorInfo {}", "TalosError::public(code, text)",
                       "content: error.to_string()"]:
            self.assertTrue(violations("gateway/response.rs", source))

    def test_accepts_projection_and_internal_diagnostic(self):
        self.assertEqual(violations("gateway/response.rs", "content: error.to_api_error().message"), [])
        self.assertEqual(violations("logging/report.rs", "content: error.to_string()"), [])
        self.assertEqual(violations("sdk/adapters.ts", 'error.code === "resourceNotFound"'), [])
        self.assertEqual(violations("sdk/adapters.ts", 'typeof error.message !== "string"'), [])
        self.assertEqual(violations("sdk/adapters.ts", 'error.message.trim().length === 0'), [])


if __name__ == "__main__":
    unittest.main()
