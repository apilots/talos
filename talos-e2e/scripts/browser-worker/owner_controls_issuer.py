"""Disposable two-account Cloud device contract inside the packaged Server container."""

from http import HTTPStatus
from html import escape
from http.server import ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import threading
import time
from urllib.parse import parse_qs, urlsplit
import uuid

from fixture import ACCOUNTS, Fixture, TOKEN

QA_TOKEN = 'browser-contract-bob'
ACCOUNTS[TOKEN] = {**ACCOUNTS[TOKEN], 'role': 'admin', 'type': 'max'}
USERS = {'Administrator': ACCOUNTS[TOKEN], 'Standard user': ACCOUNTS[QA_TOKEN]}


class DeviceIssuer(Fixture):
    # Session lookups and approval are test-only. Every product client still
    # completes its normal device flow and gets its own opaque credential.
    sessions = {}

    def do_GET(self):
        url = urlsplit(self.path)
        if url.path != '/device':
            return super().do_GET()
        query = parse_qs(url.query)
        session = query.get('session_id', [None])[0]
        record = self.sessions.get(session)
        if record is None:
            return self.reply({'error': 'unknown device session'}, HTTPStatus.NOT_FOUND)
        chosen = query.get('account', [None])[0]
        if chosen in USERS:
            token = uuid.uuid4().hex
            ACCOUNTS[token] = USERS[chosen]
            record['token'] = token
            return self.reply({'status': 'approved'})
        buttons = ''.join(f'<button name="account" value="{name}">{name}</button>' for name in USERS)
        body = (f'<form action="/device"><input type="hidden" name="session_id" '
                f'value="{escape(session)}">{buttons}</form>').encode()
        self.send_response(HTTPStatus.OK)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        if self.path not in ('/v1/auth/device/start', '/v1/auth/device/poll'):
            return super().do_POST()
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        if self.path.endswith('/start'):
            session = uuid.uuid4().hex
            code = uuid.uuid4().hex
            self.sessions[session] = {'code': code, 'token': None}
            uri = os.environ['OWNER_CONTROLS_ISSUER'] + '/device'
            return self.reply({'session_id': session, 'device_code': code, 'user_code': 'OWNER-E2E',
                'verification_uri': uri, 'verification_uri_complete': f'{uri}?session_id={session}',
                'expires_at_ms': int(time.time() * 1000) + 60_000, 'interval_ms': 100})
        record = self.sessions.get(request.get('session_id'))
        if not record or record['code'] != request.get('device_code'):
            return self.reply({'status': 'expired'})
        token = record['token']
        if token is None:
            return self.reply({'status': 'pending', 'interval_ms': 100})
        user = ACCOUNTS[token]
        self.reply({'status': 'approved', 'auth': {'access_token': token, 'user': user,
            'token_type': 'Bearer', 'expires_at_ms': int(time.time() * 1000) + 600_000,
            'capabilities': {'is_admin': user['role'] == 'admin', 'role': user['role'],
                             'scopes': ['talos:runtime']}}})


if __name__ == '__main__':
    # Server registration persists its generated credential under TALOS_HOME.
    config = Path(os.environ['TALOS_CONFIG'])
    config.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile('/fixture/talos.yaml', config)
    issuer = ThreadingHTTPServer(('0.0.0.0', 18080), DeviceIssuer)
    threading.Thread(target=issuer.serve_forever, daemon=True).start()
    raise SystemExit(subprocess.call(['talos-server']))
