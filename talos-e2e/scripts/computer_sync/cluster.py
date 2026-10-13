"""Real management hosts, native/Docker Workers and Docker WSS synchronization."""
import json
import os
from pathlib import Path
import sqlite3
import subprocess
import sys
import time
import uuid
import yaml

sys.path.append(str(Path(__file__).resolve().parents[1] / 'multi_node'))
from fixture import Cluster, REPORT, ROOT, data, free_port, http, require, wait_for
sys.path.append(str(Path(__file__).resolve().parents[1] / 'computer_workspace'))
from registration import Desktop


class SyncCluster(Cluster):
    def __init__(self):
        super().__init__(worker_transport='outbound', certificate_san='IP:127.0.0.1,DNS:host.docker.internal')
        self.hub_port = free_port()
        self.containers = []
        self.desktops = {}

    def docker(self, name, image, home, extra=(), command=()):
        container = 'talos-sync-e2e-' + name + '-' + uuid.uuid4().hex[:10]
        command = ['docker', 'run', '--detach', '--name', container, '--user', f'{os.getuid()}:{os.getgid()}',
                   '--mount', f'type=bind,source={home},target=/home/talos/.talos',
                   '--env', 'HOME=/home/talos/.talos', '--env', 'TALOS_HOME=/home/talos/.talos', *extra, image, *command]
        self.containers.append(container)
        result = subprocess.run(command, check=False, capture_output=True, text=True)
        require(result.returncode == 0, f'Docker {name} startup failed: {result.stderr.strip()}')
        require(result.stdout.strip(), f'{name}: Docker did not return a container ID')
        return container

    def start_server(self):
        # G is the only Worker registry. H is a separate container and never executes commands.
        config = Path(self.server_env['TALOS_CONFIG'])
        document = yaml.safe_load(config.read_text())
        document['cloud'].update(agent_base_url=f'https://127.0.0.1:{self.rpc_port}', agent_trusted_ca_file=str(self.cert))
        document['agent_node']['control_plane']['bind_address'] = f'0.0.0.0:{self.rpc_port}'
        document['sync'] = self.sync_config()
        config.write_text(yaml.safe_dump(document, sort_keys=False))
        home = self.root / 'sync'
        home.mkdir()
        (home / 'tls.crt').write_bytes(self.cert.read_bytes())
        (home / 'tls.key').write_bytes(self.key.read_bytes())
        (home / 'config.yaml').write_text(yaml.safe_dump({
            'listener': {'bind_address': '0.0.0.0:9220', 'certificate_file': '/home/talos/.talos/tls.crt', 'private_key_file': '/home/talos/.talos/tls.key'},
            'storage': {'backend': 'sqlite', 'path': '/home/talos/.talos/hub.db', 'max_connections': 4, 'busy_timeout_ms': 5000},
            'auth': {'issuer_url': 'http://127.0.0.1:8080', 'timeout_ms': 5000, 'revalidate_interval_ms': 1000},
        }))
        # Only loopback HTTP is permitted by H. Bridge to real Cloud Auth inside
        # the fixture namespace instead of weakening production TLS validation.
        proxy = self.docker('cloud-bridge', 'node:24-bookworm', home,
            ['--publish', f'127.0.0.1:{self.hub_port}:9220', '--entrypoint', 'node'],
            command=['-e', 'const net=require("node:net"); net.createServer(socket=>{'
                f'const upstream=net.connect({self.cloud_port},"host.docker.internal");'
                'socket.pipe(upstream).pipe(socket); upstream.on("error",()=>socket.destroy());'
                'socket.on("error",()=>upstream.destroy());}).listen(8080,"127.0.0.1");'])
        self.hub_container = self.docker('hub', os.environ.get('TALOS_SYNC_E2E_IMAGE', 'talos-sync:sync-e2e'), home,
                                         ['--network', 'container:' + proxy], command=['--config', '/home/talos/.talos/config.yaml'])
        super().start_server()

    def sync_config(self):
        return {'enabled': None, 'hub_endpoint': f'wss://127.0.0.1:{self.hub_port}/v1/sync', 'trusted_ca_file': str(self.cert)}

    def start(self):
        self.start_control_plane()
        for name in ('Desktop-A', 'Desktop-B'):
            home = self.root / name
            self.config(home, {})
            path = home / 'conf/talos.yaml'
            document = yaml.safe_load(path.read_text())
            document['cloud'].update(agent_base_url=f'https://127.0.0.1:{self.rpc_port}', agent_trusted_ca_file=str(self.cert))
            document['desktop'] = {'connection_mode': 'cloud'}
            document['sync'] = self.sync_config()
            path.write_text(yaml.safe_dump(document, sort_keys=False))
            host = Desktop(self, home, name)
            login = host.login('alice')
            token = login['access_token']
            status = self.call(host, token, 'sync/status', None)
            node = status['localComputerId']
            require(node and str(uuid.UUID(node)) == node, 'Desktop did not enroll its embedded Worker')
            self.manage('alice', 'RenameComputer', {'node_id': node, 'display_name': name})
            self.desktops[name] = {'host': host, 'token': token, 'home': home, 'node': node}
        self.add_worker('Mac-Worker', 'alice', container=False)
        self.add_worker('Docker-Worker', 'alice', container=True)
        self.add_worker('Bob-Worker', 'bob', container=True)
        wait_for(lambda: len(self.list_nodes('alice')) == 4 and all(n.get('available') for n in self.list_nodes('alice')), 'All Alice Workers available')
        for desktop in self.desktops.values():
            wait_for(lambda d=desktop: self.call(d['host'], d['token'], 'sync/status', None)['connection'] == 'connected', 'Desktop WSS connected')
        models = data(http(self.url + '/v1/models?nodeId=' + self.nodes['Mac-Worker']['node_id'], token=self.users['alice']['access_token']))['models']
        wanted = os.environ.get('TALOS_E2E_PREFERRED_MODELS', 'deepseek-v4-flash').split(',')
        self.model = next((model for identifier in wanted for model in models if model['id'] == identifier), models[0])

    @staticmethod
    def call(host, token, method, params=None):
        response = host.request(method, params, token)
        require('result' in response, f'{method}: {response.get("error", {})}')
        return response['result']

    def desktop(self, name, method, params=None):
        desktop = self.desktops[name]
        return self.call(desktop['host'], desktop['token'], method, params)

    def list_nodes(self, user):
        return data(http(self.url + '/v1/computers', token=self.users[user]['access_token']))['data']

    def start_review_ui(self):
        """Optional observer UI for this real E2E topology; no execution is mocked."""
        port = free_port()
        self.start_process('review-web', ['pnpm', '--dir', str(ROOT / 'talos-app/web'), 'exec', 'vite', '--host', '127.0.0.1', '--port', str(port), '--strictPort'],
                           dict(os.environ, VITE_API_PROXY_TARGET=self.url))
        credentials = self.root / 'review-auth.json'
        credentials.write_text(json.dumps(self.users['alice']))
        credentials.chmod(0o600)
        state = {'root': str(self.root), 'web': f'http://127.0.0.1:{port}', 'api': self.url, 'cloud': self.cloud_url,
                 'computers': {**{name: d['node'] for name, d in self.desktops.items()}, **{name: n['node_id'] for name, n in self.nodes.items()}}}
        (REPORT / 'review-state.json').write_text(json.dumps(state, indent=2))
        print('DETAIL review_ui=' + state['web'] + '/computers', flush=True)

    def add_worker(self, name, owner, *, container):
        direct = name == "Mac-Worker"
        worker_port = free_port() if direct else None
        params = {"display_name": name, "credential_expires_at": int(time.time()) + 3600}
        if direct:
            params["endpoint"] = f"https://127.0.0.1:{worker_port}"
        created = self.manage(owner, 'CreateNode', params)['created']
        node = created['node']['node_id']
        home = self.root / name
        self.config(home, {})
        credential = home / 'conf/node.credential'
        credential.write_text(created['credential']['token'])
        credential.chmod(0o600)
        ca = home / 'conf/tls.crt'
        ca.write_bytes(self.cert.read_bytes())
        base = '/home/talos/.talos' if container else str(home)
        endpoint = f'https://host.docker.internal:{self.rpc_port}' if container else f'https://127.0.0.1:{self.rpc_port}'
        path = home / 'conf/talos.yaml'
        document = yaml.safe_load(path.read_text())
        document['cloud'].update(api_base_url=f'http://host.docker.internal:{self.cloud_port}' if container else self.cloud_url,
                                 agent_base_url=endpoint, agent_trusted_ca_file=base + '/conf/tls.crt')
        document['agent_node'] = {'runner': {'node_id': node, 'control_plane_endpoint': endpoint, 'credential_file': base + '/conf/node.credential',
                                           'trusted_ca_file': base + '/conf/tls.crt', 'transport': 'direct' if direct else 'outbound'},
                                  'worker_rpc': {'bind_address': f'127.0.0.1:{worker_port}' if direct else '127.0.0.1:0'}}
        if direct:
            key = home / 'conf/tls.key'
            key.write_bytes(self.key.read_bytes())
            key.chmod(0o600)
            document['agent_node']['worker_rpc']['tls'] = {
                'certificate_file': str(ca), 'private_key_file': str(key)}
        path.write_text(yaml.safe_dump(document, sort_keys=False))
        record = {'node_id': node, 'owner': owner, 'home': home}
        if container:
            record['container'] = self.docker(name.lower(), os.environ.get('TALOS_WORKER_E2E_IMAGE', 'talos-worker:sync-e2e'), home,
                                               ['--env', 'TALOS_CONFIG=/home/talos/.talos/conf/talos.yaml'])
        else:
            record['process'] = self.start_process(name, [str(ROOT / 'talos-rs/target/debug/talos-worker')],
                                                    dict(os.environ, HOME=str(home), TALOS_HOME=str(home), TALOS_CONFIG=str(path), TALOS_RESOURCE_DIR=str(ROOT)))
        self.nodes[name] = record
        def available():
            result = self.manage(owner, 'GetNode', {'node_id': node})
            # Readiness polling may observe an explicitly unavailable Auth service.
            # Authorization failures and malformed replies still fail immediately.
            if result.get('failure', {}).get('error', {}).get('code') == 'ERROR_CODE_SERVICE_UNAVAILABLE':
                return False
            require('node' in result, f'GetNode failed: {result}')
            return result['node'].get('available')
        wait_for(available, name + (' direct registration' if direct else ' outbound registration'))

    def history(self, host, session):
        if host == 'G':
            response = http(self.url + '/v1/sessions/' + session + '/history', token=self.users['alice']['access_token'])
            if response.get('error', {}).get('code') == 'resourceNotFound':
                return {'data': []}
            return data(response)
        desktop = self.desktops[host]
        response = desktop['host'].request('session/history', {'sessionId': session, 'cursor': None, 'limit': None}, desktop['token'])
        if response.get('error', {}).get('code') == 'resourceNotFound':
            return {'data': []}
        require('result' in response, f'Session history failed: {response.get("error", {})}')
        return response['result']

    @staticmethod
    def database(home, query, values=()):
        with sqlite3.connect(f'file:{home}/data/talos.db?mode=ro', uri=True) as database:
            return database.execute(query, values).fetchall()

    def close(self):
        for container in reversed(self.containers):
            with (REPORT / (container + '.log')).open('w') as log:
                subprocess.run(['docker', 'logs', container], stdout=log, stderr=subprocess.STDOUT)
            subprocess.run(['docker', 'rm', '--force', container], check=False, capture_output=True)
        for desktop in self.desktops.values():
            desktop['host'].close()
        super().close()
