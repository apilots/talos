from pathlib import Path
from tempfile import TemporaryDirectory
import json
import tomllib
import unittest
from unittest.mock import patch

from prepare_native import PackageTaskId, Packager, copy_with_prepared_images, prepare
from summarize import write_json


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

    def test_mixed_job_preserves_hub_selection_and_supplemental_artifacts(self):
        with TemporaryDirectory() as directory:
            run = Path(directory)
            source = run / "upstream"
            source.mkdir()
            config = '[verifier.environment]\ndocker_image = "upstream/verifier:latest"\n'
            (source / "task.toml").write_text(config)
            (source / "instruction.md").write_text("Produce the answer.")
            digest, _ = Packager.compute_content_hash(source)
            registry = run / "registry.json"
            write_json(registry, [{
                "source": "example/dataset",
                "digest": "sha256:dataset",
                "tasks": [
                    {"name": "example/adapted", "digest": f"sha256:{digest}", "arm64_rebuild_verifier": True},
                    {"name": "example/ordinary", "extra_artifacts": ["/app/answer.txt"]},
                ],
            }])
            with patch.object(PackageTaskId, "get_local_path", return_value=source):
                prepare(registry, run, "example/adapted,example/ordinary")

            job = json.loads((run / "native-job.json").read_text())
            self.assertEqual(job["datasets"], [{
                "name": "example/dataset", "ref": "sha256:dataset", "task_names": ["example/ordinary"],
            }])
            self.assertEqual(job["artifacts"], ["/app/answer.txt"])
            native = Path(job["tasks"][0]["path"])
            self.assertEqual(tomllib.loads((native / "task.toml").read_text()), {"verifier": {"environment": {}}})
            provenance = json.loads((run / "provenance/adapted.json").read_text())
            self.assertEqual(provenance["upstreamTaskRef"], f"example/adapted@sha256:{digest}")


if __name__ == "__main__":
    unittest.main()
