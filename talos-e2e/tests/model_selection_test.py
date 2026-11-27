import json
import os
from pathlib import Path
import subprocess
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[2]
COMMON = ROOT / "talos-e2e/scripts/common.sh"


class ModelSelectionTests(unittest.TestCase):
    def setUp(self):
        config = yaml.safe_load((ROOT / "conf/talos-dev.yaml").read_text())
        self.models = [
            dict(model, provider=provider)
            for provider in ("anthropic", "deepseek")
            for model in config["llm"][provider]["models"]
        ]

    def select(self, function, payload, preferred=None):
        environment = {"PATH": os.environ["PATH"]}
        if preferred is not None:
            environment["TALOS_E2E_PREFERRED_MODELS"] = preferred
        result = subprocess.run(
            ["bash", "-c", 'source "$1"; "$2" "$3"', "--", str(COMMON), function, json.dumps(payload)],
            cwd=ROOT,
            env=environment,
            check=True,
            capture_output=True,
            text=True,
        )
        return result.stdout.strip()

    def test_default_prefers_current_flash_over_the_first_provider(self):
        for function, payload in [
            ("select_e2e_model_from_json", {"status": "success", "data": {"models": self.models}}),
            ("select_e2e_model_from_cli_json", {"models": self.models}),
        ]:
            with self.subTest(function=function):
                self.assertEqual(self.select(function, payload), "deepseek-flash")

    def test_explicit_preference_overrides_the_default(self):
        for function, payload in [
            ("select_e2e_model_from_json", {"status": "success", "data": {"models": self.models}}),
            ("select_e2e_model_from_cli_json", {"models": self.models}),
        ]:
            with self.subTest(function=function):
                self.assertEqual(self.select(function, payload, "deepseek-v4-pro"), "deepseek-v4-pro")


if __name__ == "__main__":
    unittest.main()
