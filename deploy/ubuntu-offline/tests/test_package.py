"""Portable delivery regressions; Docker behavior is covered by verify-bundle.py."""

import importlib.util
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import threading
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))
from package import digest, package

spec = importlib.util.spec_from_file_location('verify_bundle', SCRIPTS / 'verify-bundle.py')
verify_bundle = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify_bundle)
REVISION = '12345678' + 'a' * 32


class PackageTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.output = Path(self.temporary.name)
        for filename in ('run.sh', 'chromium-seccomp.json',
                         'talos-ubuntu-pre-arm64.tar.gz', 'talos-ubuntu-pre-amd64.tar.gz'):
            (self.output / filename).write_bytes(b'fixture:' + filename.encode())
        (self.output / 'image-prefix.txt').write_text('custom-image:' + REVISION[:8] + '\n')

    def test_dual_delivery_unpacks_with_complete_checksums_and_source(self):
        deliveries = package(self.output, REVISION, ['arm64', 'amd64'])
        self.assertEqual(len(deliveries), 3)
        for filename in deliveries:
            with self.subTest(filename=filename), tempfile.TemporaryDirectory() as scratch:
                architectures = verify_bundle.unpack(self.output / filename, Path(scratch))
                self.assertEqual(len(architectures), 2 if 'dual-arch' in filename else 1)
                self.assertEqual((Path(scratch) / 'source-revision.txt').read_text().strip(), REVISION)
                with tarfile.open(self.output / filename) as source:
                    self.assertTrue(all(member.isfile() for member in source.getmembers()))
                    self.assertFalse(any(name.startswith('._') for name in source.getnames()))
        manifest = json.loads((self.output / 'bundle-manifest.json').read_text())
        self.assertEqual(manifest['revision'], REVISION)
        for item in manifest['files']:
            self.assertEqual(item['sha256'], digest(self.output / item['file']))

    def test_single_target_excludes_stale_other_architecture_outputs(self):
        package(self.output, REVISION, ['arm64', 'amd64'])
        deliveries = package(self.output, REVISION, ['arm64'])
        self.assertEqual(deliveries, ['talos-ubuntu-pre-arm64-offline.tar'])
        manifest = json.loads((self.output / 'bundle-manifest.json').read_text())
        self.assertEqual(manifest['architectures'], ['arm64'])
        self.assertNotIn('amd64', (self.output / 'SHA256SUMS').read_text())
        self.assertNotIn('dual-arch', (self.output / 'SHA256SUMS').read_text())

    def test_revision_mismatch_is_rejected_before_writing_delivery(self):
        with self.assertRaisesRegex(ValueError, 'prefix does not match'):
            package(self.output, 'b' * 40, ['arm64'])
        self.assertFalse((self.output / 'source-revision.txt').exists())

    def test_modified_payload_is_rejected_during_unpack(self):
        package(self.output, REVISION, ['arm64'])
        (self.output / 'talos-ubuntu-pre-arm64.tar.gz').write_bytes(b'tampered')
        files = ['talos-ubuntu-pre-arm64.tar.gz', 'chromium-seccomp.json', 'run.sh',
                 'image-prefix.txt', 'source-revision.txt', 'README.txt', 'SHA256SUMS-arm64']
        archive = self.output / 'tampered.tar'
        with tarfile.open(archive, 'w') as target:
            for filename in files:
                target.add(self.output / filename, arcname=filename)
        with tempfile.TemporaryDirectory() as scratch:
            with self.assertRaisesRegex(ValueError, 'Checksum mismatch'):
                verify_bundle.unpack(archive, Path(scratch))

    def test_bundle_probe_requests_explicit_root_path(self):
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *_):
                pass

            def do_GET(self):
                body = json.dumps({"auth_enabled": self.server.auth_enabled}).encode() if self.path == '/health' else b'<html>/assets/app.js</html>'
                self.send_response(200)
                self.send_header('Content-Length', str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        try:
            for auth in (False, True):
                with self.subTest(auth=auth):
                    server.auth_enabled = auth
                    with patch.object(verify_bundle, 'run', return_value=f'127.0.0.1:{server.server_port}'), \
                            patch.object(verify_bundle.time, 'sleep', side_effect=AssertionError('Unexpected HTTP retry')):
                        verify_bundle.wait_ready('fixture', expected_auth=auth)
        finally:
            server.shutdown()
            server.server_close()
            worker.join(timeout=5)


if __name__ == '__main__':
    unittest.main()
