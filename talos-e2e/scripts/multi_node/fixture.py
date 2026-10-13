"""Real Server/Worker fixture; all mutations enter public HTTP/gRPC APIs."""

import json
import os
from pathlib import Path
import selectors
import shutil
import signal
import socket
import sqlite3
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
import uuid

import grpc
from google.protobuf import descriptor_pb2, descriptor_pool, json_format, message_factory
import yaml

ROOT = Path(__file__).resolve().parents[3]
REPORT = Path(os.environ['TALOS_SCENARIO_REPORT_DIR'])


def require(condition, message):
    if not condition:
        raise AssertionError(message)


def wait_for(probe, description, seconds=40):
    deadline = time.monotonic() + seconds
    last_error = None
    while time.monotonic() < deadline:
        try:
            result = probe()
            if result:
                return result
        except (OSError, grpc.RpcError) as error:
            last_error = type(error).__name__
        time.sleep(0.25)
    raise AssertionError(f'{description} timed out; last transport error={last_error}')


def free_port():
    with socket.socket() as listener:
        listener.bind(('127.0.0.1', 0))
        return listener.getsockname()[1]


def http(url, body=None, token=None, timeout=30, *, method=None):
    headers = {'Content-Type': 'application/json'}
    if token:
        headers['Authorization'] = f'Bearer {token}'
    request = urllib.request.Request(url, data=None if body is None else json.dumps(body).encode(), headers=headers, method=method)
    try:
        response = urllib.request.build_opener(urllib.request.ProxyHandler({})).open(request, timeout=timeout)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return json.load(response)


def data(response):
    require(response.get('status') == 'success', f'HTTP API failed: {response}')
    return response['data']


class Cluster:
    def __init__(self, dream_maintenance=False, worker_transport="direct", certificate_san="IP:127.0.0.1"):
        require(worker_transport in ("direct", "outbound"), "Invalid Worker transport")
        self.worker_transport = worker_transport
        self.dream_maintenance = dream_maintenance
        REPORT.mkdir(parents=True, exist_ok=True)
        temporary = ROOT / 'talos-e2e/.tmp'
        temporary.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix='multi-node-', dir=temporary))
        self.processes = []
        self.nodes = {}
        self.users = {}
        self.channels = []
        self.http_port, self.rpc_port, self.cloud_port = (free_port() for _ in range(3))
        self.url = f'http://127.0.0.1:{self.http_port}'
        self.cloud_url = f'http://127.0.0.1:{self.cloud_port}'
        self.server_home = self.root / 'server'
        self.cert = self.root / 'tls.crt'
        self.key = self.root / 'tls.key'
        subprocess.run(['openssl', 'req', '-x509', '-newkey', 'rsa:2048', '-nodes',
                        '-keyout', str(self.key), '-out', str(self.cert), '-days', '1',
                        '-subj', '/CN=127.0.0.1', '-addext', 'subjectAltName=' + certificate_san,
                        '-addext', 'basicConstraints=critical,CA:FALSE',
                        '-addext', 'keyUsage=critical,digitalSignature,keyEncipherment',
                        '-addext', 'extendedKeyUsage=serverAuth'], check=True, capture_output=True)
        proto = ROOT / 'talos-rs/crates/orchestration/agent-pb/proto'
        descriptor = self.root / 'agent.pb'
        subprocess.run(['protoc', f'-I{proto}', '--include_imports', f'--descriptor_set_out={descriptor}',
                        'node.proto', 'harness.proto', 'resources.proto'], check=True)
        self.pool = descriptor_pool.DescriptorPool()
        for item in descriptor_pb2.FileDescriptorSet.FromString(descriptor.read_bytes()).file:
            self.pool.Add(item)
        self.channel = self.make_channel(self.rpc_port)

    def make_channel(self, port):
        channel = grpc.secure_channel(f'127.0.0.1:{port}', grpc.ssl_channel_credentials(self.cert.read_bytes()),
                                      options=(('grpc.enable_http_proxy', 0),))
        self.channels.append(channel)
        return channel

    def rpc(self, service, method, body, token=None, channel=None):
        descriptor = self.pool.FindMethodByName(f'talos.agent.v1.{service}.{method}')
        request_cls = message_factory.GetMessageClass(descriptor.input_type)
        response_cls = message_factory.GetMessageClass(descriptor.output_type)
        request = json_format.ParseDict(body, request_cls())
        call = (channel or self.channel).unary_unary(
            f'/talos.agent.v1.{service}/{method}',
            request_serializer=request_cls.SerializeToString,
            response_deserializer=response_cls.FromString)
        result = call(request, timeout=15, metadata=() if token is None else (('authorization', f'Bearer {token}'),))
        return json_format.MessageToDict(result, preserving_proto_field_name=True)

    def manage(self, user, method, body):
        return self.rpc('AgentNodeManagement', method, body, self.users[user]['access_token'])

    def start_process(self, name, argv, env=None):
        with (REPORT / f'{name}.log').open('ab') as output:
            process = subprocess.Popen(argv, env=env, stdout=output, stderr=subprocess.STDOUT)
        self.processes.append(process)
        return process

    @staticmethod
    def stop(process):
        if process.poll() is not None:
            return
        process.send_signal(signal.SIGINT)
        try:
            process.wait(timeout=12)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)

    def config(self, home, node_config):
        conf = home / 'conf'
        conf.mkdir(parents=True, exist_ok=True)
        document = yaml.safe_load((ROOT / 'conf/talos-dev.yaml').read_text())
        document['gateway'].update(port=self.http_port, host='127.0.0.1', auth=True)
        document['cloud'].update(api_base_url=self.cloud_url, web_base_url=self.cloud_url,
                                 agent_base_url=f'https://127.0.0.1:{self.rpc_port}', agent_trusted_ca_file=str(self.cert))
        document['agent_node'] = node_config
        if self.dream_maintenance:
            timezone = 'Pacific/Honolulu' if 'control_plane' in node_config else 'Pacific/Kiritimati'
            document['timezone'] = timezone
            document['agent']['kernel']['timezone'] = timezone
        if self.dream_maintenance and 'control_plane' in node_config:
            document.setdefault('automation', {})['dream_memory_maintenance'] = {
                'cron': '0/15 * * * * *', 'batch_size': 10, 'max_batches': 1}
        path = conf / 'talos.yaml'
        path.write_text(yaml.safe_dump(document, sort_keys=False))
        if (ROOT / '.env').is_file():
            shutil.copyfile(ROOT / '.env', home / '.env')
            (home / '.env').chmod(0o600)
        return dict(os.environ, TALOS_HOME=str(home), TALOS_CONFIG=str(path), TALOS_RESOURCE_DIR=str(ROOT))

    def start_control_plane(self):
        """Start real Auth and Server without requiring execution Workers or models."""
        cloud_root = Path(os.environ.get('TALOS_CLOUD_REPO', ROOT.parent / 'talos-cloud'))
        cloud_config = self.root / 'cloud.yaml'
        cloud_config.write_text(yaml.safe_dump({
            'server': {'bind_addr': f'127.0.0.1:{self.cloud_port}', 'api_base_url': self.cloud_url, 'cloud_base_url': self.cloud_url},
            'database': {'type': 'sqlite', 'url': f'sqlite://{self.root}/cloud.db?mode=rwc'}}))
        self.start_process('cloud', [str(cloud_root / 'cloud-rs/target/debug/cloud-server')],
                           dict(os.environ, TALOS_CLOUD_CONFIG_PATH=str(cloud_config)))
        wait_for(lambda: http(self.cloud_url + '/v1/auth/config'), 'Cloud startup')
        # Cloud gives the first account admin privileges; owner-isolation cases
        # need two subsequent regular users, not an administrator.
        for user in ('admin', 'alice', 'bob'):
            name = f'{user}-{uuid.uuid4().hex[:16]}'
            password = uuid.uuid4().hex + 'Aa1!'
            registered = http(self.cloud_url + '/v1/auth/register', {'email': f'{name}@example.com', 'username': name, 'password': password})
            require('error' not in registered, f'Cloud registration failed: {registered}')
            self.users[user] = http(self.cloud_url + '/v1/auth/login', {'identifier': name, 'password': password})
            require('access_token' in self.users[user], 'Cloud login failed; inspect Cloud log')
        self.server_env = self.config(self.server_home, {'control_plane': {
            'bind_address': f'127.0.0.1:{self.rpc_port}', 'tls_certificate_file': str(self.cert),
            'tls_private_key_file': str(self.key), 'trusted_node_ca_file': str(self.cert)}})
        self.start_server()

    def start(self):
        self.start_control_plane()
        # An existing control-plane directory must not override a remote node's
        # implicit working directory. Empty directories also reproduce the bug.
        for user in ('alice', 'bob'):
            (self.server_home / 'workspace-talos' / self.users[user]['user']['user_id']).mkdir(parents=True, exist_ok=True)
        for name, owner in (('alice-a', 'alice'), ('alice-b', 'alice'), ('bob-a', 'bob')):
            port = free_port()
            response = self.manage(owner, 'CreateNode', {
                'display_name': name, **({'endpoint': f'https://127.0.0.1:{port}'} if self.worker_transport == 'direct' else {}),
                'credential_expires_at': int(time.time()) + 3600})
            require('created' in response, f'CreateNode failed: {response}')
            node_id = response['created']['node']['node_id']
            require(str(uuid.UUID(node_id)) == node_id, 'Control plane did not issue a UUID')
            home = self.root / name
            credential = home / 'conf/node.credential'
            runner = {'node_id': node_id, 'control_plane_endpoint': f'https://127.0.0.1:{self.rpc_port}',
                      'credential_file': str(credential), 'trusted_ca_file': str(self.cert)}
            runner['transport'] = self.worker_transport
            worker_rpc = {'bind_address': f'127.0.0.1:{port}'}
            if self.worker_transport == 'direct':
                worker_rpc['tls'] = {'certificate_file': str(self.cert), 'private_key_file': str(self.key)}
            env = self.config(home, {'runner': runner, 'worker_rpc': worker_rpc})
            credential.write_text(response['created']['credential']['token'])
            credential.chmod(0o600)
            self.nodes[name] = dict(node_id=node_id, owner=owner, home=home, port=port, env=env, credential=credential)
            self.start_worker(name)
        # Aliases keep scenarios readable; routing always compares the issued UUIDs.
        if self.nodes['alice-a']['node_id'] > self.nodes['alice-b']['node_id']:
            self.nodes['alice-a'], self.nodes['alice-b'] = self.nodes['alice-b'], self.nodes['alice-a']
        self.healthy_all()
        models = data(http(self.url + '/v1/models?nodeId=' + self.nodes['alice-a']['node_id'], token=self.users['alice']['access_token']))['models']
        preferred = os.environ.get('TALOS_E2E_PREFERRED_MODELS', 'deepseek-v4-flash').split(',')
        self.model = next((model for identifier in preferred for model in models if model['id'] == identifier), models[0])

    def start_server(self):
        self.server = self.start_process('server', [str(ROOT / 'talos-rs/target/debug/talos-server')], self.server_env)
        wait_for(lambda: http(self.url + '/health'), 'Server startup')
        grpc.channel_ready_future(self.channel).result(timeout=20)

    def start_worker(self, name):
        node = self.nodes[name]
        node['process'] = self.start_process(name, [str(ROOT / 'talos-rs/target/debug/talos-worker')], node['env'])

    def info(self, name):
        result = self.manage(self.nodes[name]['owner'], 'GetNode', {'node_id': self.nodes[name]['node_id']})
        require('node' in result, f'GetNode failed: {result}')
        return result['node']

    def healthy_all(self):
        for name in self.nodes:
            wait_for(lambda name=name: self.info(name).get('available'), f'{name} available')

    def query(self, sql, args=()):
        # Read-only observations; all product mutations use public APIs.
        with sqlite3.connect(f'file:{self.server_home}/data/talos.db?mode=ro', uri=True) as connection:
            return connection.execute(sql, args).fetchall()

    def start_turn(self, user, prompt, session=None, file_ids=(), node_id=None, project_id=None):
        if node_id is None and session is None:
            node_id = next(node['node_id'] for node in self.nodes.values() if node['owner'] == user)
        session = session or f'sess_{uuid.uuid4().hex}'
        response = http(self.url + '/v1/turns/start', {
            'sessionId': session, **({'nodeId': node_id} if node_id is not None else {}), **({'projectId': project_id} if project_id is not None else {}), 'modelSelection': {'modelRef': {'provider': self.model['provider'], 'id': self.model['id']}, 'reasoningEffort': None},
            'userPermissionMode': 'fullAccess', 'input': {'content': [{'type': 'text', 'text': prompt}] + [{'type': 'file', 'fileId': file_id} for file_id in file_ids]}},
            self.users[user]['access_token'])
        return session, response

    def turn(self, user, prompt, session=None, timeout=90, node_id=None, project_id=None):
        session, response = self.start_turn(user, prompt, session, node_id=node_id, project_id=project_id)
        ack = data(response)
        events = self.events(user, session, ack['turnId'], timeout)
        require(events[-1]['payload']['type'] == 'turnCompleted', f'Turn did not complete: {events[-1]["payload"]}')
        sequences = [event['sequence'] for event in events]
        require(sequences == list(range(sequences[0], sequences[-1] + 1)), f'RPC lost progress events: {sequences}')
        text = ''.join(block['content']['data']['text'] for event in events
                       if event['payload']['type'] == 'messageCompleted'
                       for block in event['payload']['data']['blocks']
                       if block['content']['type'] == 'assistantText' and block['content']['data']['phase'] == 'finalAnswer')
        return session, text, events

    def events(self, user, session, turn, timeout, after=None):
        token = self.users[user]['access_token']
        url = f'{self.url}/v1/sessions/{session}/events?limit=10000&turn_id={turn}'
        if after:
            url += '&after_event_id=' + urllib.parse.quote(after)
        process = subprocess.Popen(['curl', '-fsSN', '--max-time', str(timeout), '-H', f'Authorization: Bearer {token}', url],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        events, pending = [], b''
        started = time.monotonic()
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        try:
            with (REPORT / f'{session}_{turn}{"_resumed" if after else ""}.jsonl').open('w') as log:
                while time.monotonic() - started < timeout:
                    if not selector.select(timeout=0.5):
                        continue
                    chunk = os.read(process.stdout.fileno(), 65536)
                    if not chunk:
                        active = self.query('SELECT state FROM agent_active_turns WHERE session_id = ?', (session,))
                        raise AssertionError(f'SSE ended without terminal event; curl_exit={process.wait(timeout=5)}, active_turn={active}')
                    pending += chunk
                    while b'\n\n' in pending:
                        frame, pending = pending.split(b'\n\n', 1)
                        for line in frame.splitlines():
                            if not line.startswith(b'data: '):
                                continue
                            record = json.loads(line[6:])
                            log.write(json.dumps({'received_after_seconds': round(time.monotonic() - started, 3), 'record': record}) + '\n')
                            log.flush()
                            if record.get('event', {}).get('type') != 'agent':
                                continue
                            event = record['event']['data']
                            require(event['identity']['turnId'] == turn, 'Unexpected Turn in SSE')
                            events.append(event)
                            if event['payload']['type'] in ('turnCompleted', 'turnFailed', 'waitingForInput'):
                                return events
                active = self.query('SELECT state FROM agent_active_turns WHERE session_id = ?', (session,))
                raise AssertionError(f'No terminal event within {timeout}s; active_turn={active}; inspect Server/Worker logs')
        finally:
            selector.close()
            if process.poll() is None:
                process.terminate()
            process.communicate(timeout=5)

    def verify_file(self, name, filename, marker):
        matches = list(self.nodes[name]['home'].rglob(filename))
        require(len(matches) == 1 and matches[0].read_text() == marker, f'{name}: missing or incorrect {filename}')
        for other, node in self.nodes.items():
            if other != name:
                require(not list(node['home'].rglob(filename)), f'{filename} leaked to {other}')
        require(not list(self.server_home.rglob(filename)), f'{filename} was written on control plane')

    def close(self):
        for process in reversed(self.processes):
            self.stop(process)
        for channel in self.channels:
            channel.close()
        if os.environ.get('TALOS_MULTI_NODE_KEEP_TMP') == '1':
            print(f'DETAIL retained_fixture={self.root}', flush=True)
        else:
            shutil.rmtree(self.root)
