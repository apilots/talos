"""Stateless legacy MCP peer: verifies real HTTP credentials without logging their values."""
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

root = Path(sys.argv[1])

class Peer(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def reply(self, status, body=None):
        data = json.dumps(body).encode() if body is not None else b''
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.send_header('Content-Length', str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        self.reply(405)

    def do_DELETE(self):
        self.reply(200)

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        mode = self.path.strip('/')
        expected = (root / 'expected').read_text()
        header = self.headers.get('Authorization') if mode == 'bearer' else self.headers.get('X-API-Key')
        if header != ('Bearer ' + expected if mode == 'bearer' else expected):
            self.reply(401)
            return
        if 'id' not in request:
            self.reply(202)
            return
        response = dict(jsonrpc='2.0', id=request['id'])
        method = request['method']
        if method == 'initialize':
            response['result'] = dict(protocolVersion='2025-11-25', capabilities=dict(tools={}), serverInfo=dict(name='http-fixture', version='1'))
        elif method == 'tools/list':
            response['result'] = dict(tools=[dict(name='inspect', inputSchema=dict(type='object'))])
        elif method == 'tools/call':
            (root / 'called').write_text(mode)
            response['result'] = dict(content=[dict(type='text', text='authenticated')])
        else:
            response['error'] = dict(code=-32601, message='legacy peer')
        self.reply(200, response)

if __name__ == '__main__':
    server = ThreadingHTTPServer(('127.0.0.1', 0), Peer)
    (root / 'port').write_text(str(server.server_port))
    server.serve_forever()
