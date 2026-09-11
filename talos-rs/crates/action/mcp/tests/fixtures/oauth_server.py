"""Local issuer plus real MCP transport; records grant constraints, never prints tokens."""
import base64
import hashlib
import json
import time
from urllib.parse import urlparse, parse_qs, urlencode
from http.server import ThreadingHTTPServer
from http_server import Peer, root

codes = {}
refreshes = 0

class OAuthPeer(Peer):
    def do_GET(self):
        path = urlparse(self.path).path
        if path.startswith('/.well-known/oauth-protected-resource'):
            if (root / 'hold-discovery').exists():
                (root / 'discovery-started').write_text('waiting')
                while (root / 'hold-discovery').exists():
                    time.sleep(.01)
            self.reply(200, dict(resource=origin + ('/' if (root / 'changed-resource').exists() else '/bearer'), authorization_servers=[origin], scopes_supported=['read', 'admin']))
        elif path.startswith('/.well-known/oauth-authorization-server'):
            self.reply(200, dict(issuer=origin, authorization_endpoint=origin + '/authorize', token_endpoint=origin + '/token', registration_endpoint=origin + '/register', response_types_supported=['code'], grant_types_supported=['authorization_code', 'refresh_token'], token_endpoint_auth_methods_supported=['none'], code_challenge_methods_supported=['S256'], scopes_supported=['read', 'admin'], authorization_response_iss_parameter_supported=True))
        elif path == '/authorize':
            params = parse_qs(urlparse(self.path).query)
            assert params['resource'] == [origin + '/bearer']
            assert params['scope'] == ['read']
            assert params['code_challenge_method'] == ['S256']
            code = 'code-' + str(len(codes))
            codes[code] = params
            (root / 'authorization.json').write_text(json.dumps(params))
            callback = params['redirect_uri'][0] + '?' + urlencode(dict(code=code, state=params['state'][0], iss=origin))
            self.send_response(302)
            self.send_header('Location', callback)
            self.send_header('Content-Length', '0')
            self.end_headers()
        else:
            super().do_GET()

    def do_POST(self):
        global refreshes
        if self.path == '/register':
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            assert body.get('scope') == 'read'
            self.reply(201, dict(client_id='local-client', redirect_uris=body['redirect_uris'], token_endpoint_auth_method='none'))
        elif self.path == '/token':
            body = parse_qs(self.rfile.read(int(self.headers['Content-Length'])).decode())
            assert body['resource'] == [origin + '/bearer']
            if body['grant_type'] == ['authorization_code']:
                params = codes.pop(body['code'][0])
                digest = base64.urlsafe_b64encode(hashlib.sha256(body['code_verifier'][0].encode()).digest()).decode().rstrip('=')
                assert digest == params['code_challenge'][0]
                assert body['redirect_uri'] == params['redirect_uri']
                token = 'first-private-access'
                expires = 3600
            else:
                assert body['refresh_token'] == ['private-refresh']
                refreshes += 1
                (root / 'refresh-count').write_text(str(refreshes))
                while (root / 'hold-refresh').exists():
                    time.sleep(.01)
                token = 'refreshed-private-access'
                expires = 3600
            (root / 'expected').write_text(token)
            self.reply(200, dict(access_token=token, token_type='Bearer', refresh_token='private-refresh', expires_in=expires, scope='read'))
        else:
            super().do_POST()

server = ThreadingHTTPServer(('127.0.0.1', 0), OAuthPeer)
origin = 'http://127.0.0.1:' + str(server.server_port)
(root / 'port').write_text(str(server.server_port))
(root / 'expected').write_text('unauthorized')
server.serve_forever()
