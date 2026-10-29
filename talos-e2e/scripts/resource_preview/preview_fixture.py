"""Controlled Auth/model upstreams; all sessions/files use real product APIs and storage."""
import json
from pathlib import Path
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'computer_workspace'))
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'workspace_resources'))
from deletion_auth_fixture import ContractAuthCluster
from fixture import data, http, free_port, require


class ModelReply(BaseHTTPRequestHandler):
    text = 'Preview acceptance response.'

    def log_message(self, *_args):
        pass

    def do_GET(self):
        body = b'<!doctype html><title>Local website</title><h1>Local preview website</h1>'
        self.send_response(200)
        self.send_header('Content-Type', 'text/html')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers.get('Content-Length', 0))))
        if not request.get('stream', False):
            # Auxiliary memory reflection has its own typed model response.
            reflection = any('currentMemory' in str(message.get('content', '')) for message in request.get('messages', []))
            content = '{"operation":"noop"}' if reflection else self.text.splitlines()[0]
            encoded = json.dumps({'id': 'preview-title', 'model': 'preview-model', 'choices': [
                {'index': 0, 'message': {'role': 'assistant', 'content': content}, 'finish_reason': 'stop'}],
                'usage': {'prompt_tokens': 10, 'completion_tokens': 3, 'total_tokens': 13}}).encode()
            self.send_response(200)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(encoded)))
            self.end_headers()
            self.wfile.write(encoded)
            return
        chunks = [
            {'choices': [{'index': 0, 'delta': {'role': 'assistant', 'content': self.text}, 'finish_reason': None}]},
            {'choices': [{'index': 0, 'delta': {}, 'finish_reason': 'stop'}]},
        ]
        body = ''.join('data: ' + json.dumps({'id': 'preview-acceptance', 'object': 'chat.completion.chunk',
            'created': int(time.time()), 'model': 'preview-model', **chunk}) + '\n\n' for chunk in chunks) + 'data: [DONE]\n\n'
        encoded = body.encode()
        self.send_response(200)
        self.send_header('Content-Type', 'text/event-stream')
        self.send_header('Content-Length', str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


class PreviewCluster(ContractAuthCluster):
    def __init__(self, *, format_opt_ins=True):
        self.format_opt_ins = format_opt_ins
        super().__init__()
        self.model = {'provider': 'preview-upstream', 'id': 'preview-model'}
        self.upstream = ThreadingHTTPServer(('127.0.0.1', 0), ModelReply)
        self.upstream_thread = threading.Thread(target=self.upstream.serve_forever, daemon=True)
        self.upstream_thread.start()

    def config(self, home, node_config):
        env = super().config(home, node_config)
        path = Path(env['TALOS_CONFIG'])
        config = yaml.safe_load(path.read_text())
        config['agent']['inner_model'] = self.model
        config['storage']['files']['preview_base_url'] = f'http://preview.localhost:{self.http_port}/'
        # Explicit disposable opt-in; product deployment defaults stay unchanged.
        if self.format_opt_ins:
            cases = json.loads((Path(__file__).resolve().parents[2] / 'fixtures/resource-preview/manifest.json').read_text())
            config['storage']['files']['allowed_mime_types'] = sorted(set(config['storage']['files']['allowed_mime_types']) | {case['mime'] for case in cases})
        config['llm'].update(provider=self.model['provider'], model=self.model['id'], custom_providers={
            self.model['provider']: {'name': 'Preview acceptance upstream', 'protocol': 'chat',
                'api_key': 'loopback-fixture', 'base_url': f'http://127.0.0.1:{self.upstream.server_port}/v1',
                'models': [{'id': self.model['id'], 'name': 'Preview model', 'context_window': 131072,
                    'reasoning': False, 'reasoning_effort': 'medium', 'speed': 'standard', 'input': ['text', 'image', 'file'],
                    'cost': {'input': 0, 'output': 0, 'cache_read': 0, 'cache_write': 0}}]}})
        path.write_text(yaml.safe_dump(config, sort_keys=False))
        return env

    def session(self, text, file_ids=(), worker="preview"):
        ModelReply.text = text
        node = self.nodes[worker]['node_id']
        session, response = self.start_turn('alice', text.splitlines()[0], file_ids=file_ids, node_id=node)
        ack = data(response)
        events = self.events('alice', session, ack['turnId'], timeout=60)
        require(events[-1]['payload']['type'] == 'turnCompleted', 'Controlled resource Turn failed: ' + repr(events[-1]['payload']))
        data(http(self.url + '/v1/sessions/' + session, token=self.users['alice']['access_token']))
        return session

    def start_preview_worker(self, name="preview", transport="direct"):
        port = free_port()
        response = self.manage('alice', 'CreateNode', {'display_name': 'Preview Computer',
            **({'endpoint': f'https://127.0.0.1:{port}'} if transport == 'direct' else {}), 'credential_expires_at': int(time.time()) + 3600})['created']
        node = response['node']['node_id']
        home = self.root / (name + '-worker')
        credential = home / 'conf/node.credential'
        env = self.config(home, {'runner': {'node_id': node,
            'control_plane_endpoint': f'https://127.0.0.1:{self.rpc_port}',
            'credential_file': str(credential), 'trusted_ca_file': str(self.cert), 'transport': transport},
            'worker_rpc': {'bind_address': f'127.0.0.1:{port}',
                'tls': {'certificate_file': str(self.cert), 'private_key_file': str(self.key)}}})
        credential.write_text(response['credential']['token'])
        credential.chmod(0o600)
        self.nodes[name] = dict(node_id=node, owner='alice', home=home, port=port, env=env, credential=credential)
        self.start_worker(name)
        self.healthy_all()
        require(self.info(name)['available'], 'Preview execution Worker did not register')

    def close(self):
        try:
            super().close()
        finally:
            self.upstream.shutdown()
            self.upstream.server_close()
            self.upstream_thread.join(timeout=5)
