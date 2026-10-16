import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from release_candidate import candidate_artifact, select_run, validate_evidence, validate_run


class ReleaseCandidateTests(unittest.TestCase):
    def setUp(self):
        self.sha = "a" * 40
        self.run = dict(id=42, run_attempt=1, repository={"full_name": "apilots/talos"},
                        head_repository={"full_name": "apilots/talos"}, head_sha=self.sha,
                        head_branch="release-0.2.0", path=".github/workflows/release.yml",
                        event="push", status="completed", conclusion="success")

    def test_select_exact_successful_release_branch(self):
        tagged = dict(self.run, head_branch="v0.2.0")
        self.assertEqual(select_run([tagged, self.run], "apilots/talos", self.sha), self.run)

    def test_requires_candidate_source_and_trusted_workflow(self):
        for key, value in [("head_sha", "b" * 40), ("head_branch", "main"),
                           ("path", ".github/workflows/other.yml"), ("event", "pull_request"),
                           ("head_repository", {"full_name": "fork/talos"}),
                           ("status", "in_progress"), ("conclusion", "failure")]:
            with self.subTest(key=key), self.assertRaises(ValueError):
                validate_run(dict(self.run, **{key: value}), "apilots/talos", self.sha)

    def test_latest_candidate_must_succeed(self):
        with self.assertRaises(ValueError):
            select_run([dict(self.run, conclusion="failure"), self.run],
                       "apilots/talos", self.sha)

    def test_record_candidate_lock_notes_and_source(self):
        repo = Path.cwd()
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / ".github").mkdir()
            (root / "talos-rs").mkdir()
            (root / ".github/release-toolchain.env").write_bytes(
                (repo / ".github/release-toolchain.env").read_bytes())
            (root / "talos-rs/Cargo.lock").write_text('version = 4\n')
            def git(*args):
                return subprocess.check_output(["git", *args], cwd=root, text=True).strip()
            git("init", "-q")
            git("config", "user.name", "Release Test")
            git("config", "user.email", "release-test@example.com")
            git("add", ".")
            git("commit", "-qm", "chore: initialize")
            git("tag", "v0.1.9")
            (root / "change").write_text("feature\n")
            git("add", ".")
            git("commit", "-qm", "feat: add feature")
            sha = git("rev-parse", "HEAD")
            directory = root / "candidate"
            directory.mkdir()
            subprocess.run(["bash", str(repo / "scripts/generate-release-notes.sh"),
                            "0.2.0", "--from", "v0.1.9", "--to", sha,
                            "--output", str(directory / "release-body.md")], cwd=root, check=True)
            subprocess.run([sys.executable, str(repo / ".github/scripts/release_candidate.py"),
                            "record", "--repository", "apilots/talos", "--sha", sha,
                            "--run-id", "42", "--run-attempt", "1", "--version", "0.2.0",
                            "--previous-tag", "v0.1.9", "--directory", str(directory)],
                           cwd=root, check=True)
            evidence = json.loads((directory / "candidate.json").read_text())
            self.assertEqual(evidence["source_sha"], sha)
            self.assertEqual(evidence["run_attempt"], 1)
            self.assertEqual((directory / "Cargo.lock").read_bytes(),
                             (root / "talos-rs/Cargo.lock").read_bytes())
            self.assertEqual(evidence["notes_sha256"], hashlib.sha256(
                (directory / "release-body.md").read_bytes()).hexdigest())
            (root / "talos-rs/Cargo.lock").write_text('version = 3\n')
            result = subprocess.run([
                sys.executable, str(repo / ".github/scripts/release_candidate.py"),
                "record", "--repository", "apilots/talos", "--sha", sha,
                "--run-id", "42", "--run-attempt", "1", "--version", "0.2.0",
                "--previous-tag", "v0.1.9", "--directory", str(directory)],
                cwd=root, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("Validated Cargo.lock differs from committed source", result.stderr)

    def test_requires_live_current_attempt_evidence(self):
        good = dict(id=7, name="release-candidate-1", expired=False)
        with patch("release_candidate.gh_api", return_value={"artifacts": [good]}):
            self.assertEqual(candidate_artifact("apilots/talos", self.run), good)
        for artifacts in [[], [dict(good, expired=True)], [dict(good, name="release-candidate-2")]]:
            with self.subTest(artifacts=artifacts), self.assertRaises(ValueError), \
                    patch("release_candidate.gh_api", return_value={"artifacts": artifacts}):
                candidate_artifact("apilots/talos", self.run)

    def test_verify_source_lock_toolchain_and_generated_notes(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            (directory / "Cargo.lock").write_bytes(b"locked graph")
            (directory / "release-body.md").write_bytes(b"# Talos v0.2.0\n")
            digest = lambda value: hashlib.sha256(value).hexdigest()
            evidence = dict(repository="apilots/talos", source_sha=self.sha,
                            run_id=42, run_attempt=1, version="0.2.0",
                            previous_tag="v0.1.9", previous_sha="b" * 40,
                            lock_sha256=digest(b"locked graph"),
                            toolchain_sha256=digest(Path(".github/release-toolchain.env").read_bytes()),
                            notes_sha256=digest(b"# Talos v0.2.0\n"))
            def command(args, **kwargs):
                return "b" * 40 + "\n" if args[0] == "git" else b"# Talos v0.2.0\n"
            with patch("release_candidate.subprocess.check_output", side_effect=command):
                validate_evidence(evidence, self.run, directory)
                for key, value in [("source_sha", "c" * 40), ("lock_sha256", "wrong"),
                                   ("toolchain_sha256", "wrong"), ("run_attempt", 2)]:
                    with self.subTest(key=key), self.assertRaises(ValueError):
                        validate_evidence(dict(evidence, **{key: value}), self.run, directory)
                (directory / "release-body.md").write_bytes(b"handwritten Notes")
                evidence["notes_sha256"] = digest(b"handwritten Notes")
                with self.assertRaises(ValueError):
                    validate_evidence(evidence, self.run, directory)
