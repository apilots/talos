"""Real HTTP boundary and lifecycle checks for the packaged homepage component."""
import http.client
from pathlib import Path
import select
import shutil
import socket
import subprocess
import sys
import tempfile
import unittest

from home import is_home_url


ROOT = Path(__file__).resolve().parents[3]
SERVER = ROOT / 'build/worker/browser/desktop/home_server.py'


class HomeServerTest(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        shutil.copyfile(SERVER, self.root / SERVER.name)
        self.assets = {
            '/home.html': (ROOT / 'talos-app/shared/browser-home/home.html').read_bytes(),
            '/home.js': (ROOT / 'talos-app/shared/browser-home/home.js').read_bytes(),
            '/talos-logo.svg': (ROOT / 'talos-app/web/public/talos-logo.svg').read_bytes(),
        }
        for path, content in self.assets.items():
            (self.root / path[1:]).write_bytes(content)
        # A sibling and a parent file must never become HTTP resources.
        (self.root / 'private.txt').write_text('not a homepage asset')
        self.process = subprocess.Popen([sys.executable, '-I', str(self.root / SERVER.name)],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        self.addCleanup(self.stop)
        self.assertTrue(select.select([self.process.stdout], [], [], 5)[0], 'Missing ready line')
        line = self.process.stdout.readline().decode().strip()
        self.assertTrue(line.isdecimal(), 'Ready line must contain only the private port')
        self.port = int(line)
        self.assertTrue(is_home_url(f'http://127.0.0.1:{self.port}/home.html'))

    def stop(self):
        if self.process.poll() is None:
            self.process.terminate()
            self.process.wait(timeout=5)
        self.process.stdout.close()
        self.process.stderr.close()

    def request(self, path, method='GET'):
        connection = http.client.HTTPConnection('127.0.0.1', self.port, timeout=2)
        try:
            connection.request(method, path)
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), response.read()
        finally:
            connection.close()

    def test_get_and_head_serve_only_the_three_preloaded_assets(self):
        types = {'/home.html': 'text/html; charset=utf-8',
                 '/home.js': 'text/javascript; charset=utf-8', '/talos-logo.svg': 'image/svg+xml'}
        for path, content in self.assets.items():
            for method in ('GET', 'HEAD'):
                with self.subTest(path=path, method=method):
                    status, headers, body = self.request(path, method)
                    self.assertEqual(status, 200)
                    self.assertEqual(headers['Content-Type'], types[path])
                    self.assertEqual(headers['Content-Length'], str(len(content)))
                    self.assertEqual(headers['X-Content-Type-Options'], 'nosniff')
                    self.assertEqual(body, content if method == 'GET' else b'')
        (self.root / 'home.html').write_text('changed after startup')
        self.assertEqual(self.request('/home.html')[2], self.assets['/home.html'])

    def test_paths_queries_and_write_methods_cannot_access_other_files(self):
        for path in ('/', '/private.txt', '/home_server.py', '/../private.txt',
                     '/%2e%2e/private.txt', '/home.html?path=private.txt',
                     '/home.js?cache=1', '//home.html', '/home.html/extra'):
            with self.subTest(path=path):
                self.assertEqual(self.request(path)[0], 404)
        for method in ('POST', 'PUT', 'PATCH', 'DELETE', 'OPTIONS'):
            with self.subTest(method=method):
                self.assertEqual(self.request('/home.html', method)[0], 501)
        self.assertEqual(self.request('/home.html')[2], self.assets['/home.html'])

    def test_listener_is_loopback_only_and_process_exit_releases_it(self):
        # UDP connect selects a real local interface without sending a packet.
        with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as route:
            try:
                route.connect(('192.0.2.1', 9))
                interface = route.getsockname()[0]
            except OSError:
                interface = None
        if interface and not interface.startswith('127.'):
            with self.assertRaises(OSError):
                socket.create_connection((interface, self.port), timeout=1)
        self.assertEqual(self.request('/home.html')[0], 200)
        self.stop()
        with self.assertRaises(OSError):
            socket.create_connection(('127.0.0.1', self.port), timeout=1)

    def test_canonical_url_requires_exact_loopback_origin_and_home_path(self):
        for value in ('file:///opt/talos/browser/desktop/home.html',
                      'http://localhost:1234/home.html', 'http://127.0.0.1:0/home.html',
                      'http://127.0.0.1:65536/home.html', 'http://127.0.0.1:1234/home.html?x=1',
                      'http://user@127.0.0.1:1234/home.html', 'https://127.0.0.1:1234/home.html'):
            with self.subTest(value=value):
                self.assertFalse(is_home_url(value))


if __name__ == '__main__':
    unittest.main()
