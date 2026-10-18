"""Explicit secretless Auth contract fixture; Computer APIs and storage remain real."""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "multi_node"))
from fixture import Cluster, http, wait_for

USERS = {name: {'user_id': 'e2e-' + name, 'email': name + '@example.test', 'username': name,
    'role': 'user', 'type': 'free', 'status': 'active', 'email_verified': True,
    'set_password_required': False, 'disabled': False, 'created_at_ms': 1, 'updated_at_ms': 1}
    for name in ('alice', 'bob')}
TOKENS = {'e2e-' + name: user for name, user in USERS.items()}


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        pass

    def reply(self, body, status=200):
        payload = json.dumps(body).encode()
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_GET(self):
        if self.path == '/v1/auth/config':
            return self.reply({'registration_enabled': True})
        if self.path == '/v1/me':
            user = TOKENS.get(self.headers.get('Authorization', '').removeprefix('Bearer '))
            return self.reply(user if user else {'error': 'unauthenticated'}, 200 if user else 401)
        self.reply({'error': 'not_found'}, 404)

    def do_POST(self):
        if self.path != '/v1/auth/introspect':
            return self.reply({'error': 'not_found'}, 404)
        body = json.loads(self.rfile.read(int(self.headers.get('Content-Length', 0))))
        user = TOKENS.get(body.get('token'))
        self.reply({'active': bool(user), 'user': user, 'capabilities': None,
            'entitlement': {'can_use': True, 'plan': 'free', 'reason': None} if user else None,
            'expires_at_ms': None})


class ContractAuthCluster(Cluster):
    def start_control_plane(self):
        self.users = {name: {'access_token': 'e2e-' + name, 'user': user} for name, user in USERS.items()}
        self.start_process('contract-auth', [sys.executable, str(Path(__file__).resolve()), str(self.cloud_port)])
        wait_for(lambda: http(self.cloud_url + '/v1/auth/config'), 'Auth contract fixture')
        self.server_env = self.config(self.server_home, {'control_plane': {
            'bind_address': f'127.0.0.1:{self.rpc_port}', 'tls_certificate_file': str(self.cert),
            'tls_private_key_file': str(self.key), 'trusted_node_ca_file': str(self.cert)}})
        self.start_server()


if __name__ == '__main__':
    ThreadingHTTPServer(('127.0.0.1', int(sys.argv[1])), Handler).serve_forever()
