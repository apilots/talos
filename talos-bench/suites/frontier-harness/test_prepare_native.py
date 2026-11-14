from pathlib import Path
from tempfile import TemporaryDirectory
import tomllib
import unittest

from prepare_native import copy_with_prepared_images


class PreparedTaskTest(unittest.TestCase):
    def test_prepared_images_preserve_service_artifacts_and_other_task_fields(self):
        with TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "original"
            source.mkdir()
            config = '''artifacts = ["/workspace/answer.json", { source = "/state.json", service = "db" }]

[verifier.environment]
docker_image = "upstream/verifier:latest"
memory_mb = 8192

[environment]
build_timeout_sec = 1200
'''
            (source / "task.toml").write_text(config)
            (source / "instruction.md").write_text("Perform the task.")
            native = root / "native"

            copy_with_prepared_images(
                source, native, {"agent": "local/agent:arm64", "verifier": "local/verifier:arm64"},
            )

            expected = tomllib.loads(config)
            expected["environment"]["docker_image"] = "local/agent:arm64"
            expected["verifier"]["environment"]["docker_image"] = "local/verifier:arm64"
            self.assertEqual(tomllib.loads((native / "task.toml").read_text()), expected)
            self.assertEqual((native / "instruction.md").read_bytes(), (source / "instruction.md").read_bytes())
            self.assertEqual((source / "task.toml").read_text(), config)


if __name__ == "__main__":
    unittest.main()
