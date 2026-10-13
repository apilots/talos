"""Two actual management Servers behind independent Docker NAT networks."""
import base64
import json
import os
import subprocess
import uuid
import yaml
from cluster import SyncCluster, REPORT, require, wait_for, data


def command(argv, **kwargs):
    return subprocess.run(argv, check=True, capture_output=True, **kwargs).stdout.decode().strip()


class NatHosts:
    def __init__(self, cluster):
        self.cluster = cluster
        self.hosts = {}
        self.networks = []

    def start(self, name):
        cluster = self.cluster
        home = cluster.root / name
        cluster.config(home, {})
        path = home / 'conf/talos.yaml'
        conf = yaml.safe_load(path.read_text())
        conf['gateway'].update(host='0.0.0.0', port=8080)
        if name == 'NAT-B':
            conf['automation']['dream_memory_maintenance'].update(cron='0/15 * * * * *', batch_size=10, max_batches=1)
        base = '/home/talos/.talos'
        (home / 'conf/ca.crt').write_bytes(cluster.cert.read_bytes())
        conf['cloud'].update(api_base_url=f'http://host.docker.internal:{cluster.cloud_port}',
                             agent_base_url=f'https://host.docker.internal:{cluster.rpc_port}', agent_trusted_ca_file=base + '/conf/ca.crt')
        conf['sync'] = {'hub_endpoint': f'wss://host.docker.internal:{cluster.hub_port}/v1/sync', 'trusted_ca_file': base + '/conf/ca.crt'}
        path.write_text(yaml.safe_dump(conf, sort_keys=False))
        auth = home / 'auth'
        auth.mkdir()
        credentials = auth / 'request.conf'
        credentials.write_text('silent\nshow-error\nnoproxy = "*"\nheader = "Authorization: Bearer ' + cluster.users['alice']['access_token'] + '"\n')
        credentials.chmod(0o600)
        network = 'talos-sync-nat-' + uuid.uuid4().hex[:10]
        command(['docker', 'network', 'create', network])
        self.networks.append(network)
        container = cluster.docker(name.lower(), os.environ.get('TALOS_SERVER_E2E_IMAGE', 'talos-server:sync-e2e'), home,
                                  ['--network', network, '--env', 'TALOS_CONFIG=' + base + '/conf/talos.yaml'])
        address = json.loads(command(['docker', 'inspect', container]))[0]['NetworkSettings']['Networks'][network]['IPAddress']
        self.hosts[name] = {'container': container, 'home': home, 'address': address}
        wait_for(lambda: self.healthy(name), name + ' original HTTP API')
        self.firewall(name)
        wait_for(lambda: data(self.api(name, '/v1/sync/status'))['connection'] == 'connected', name + ' authenticated WSS')
        wait_for(lambda: len(data(self.api(name, '/v1/computers'))['data']) == 4, name + ' same-account directory imported')

    def firewall(self, name):
        command(['docker', 'run', '--rm', '--network', 'container:' + self.hosts[name]['container'], '--cap-add', 'NET_ADMIN',
                 'talos-e2e-firewall:local'])

    def database(self, name, query, values=()):
        # Keep readers in the same Linux locking domain as the live WAL writer.
        source = "import json,sqlite3,sys; db=sqlite3.connect('file:/data/data/talos.db?mode=ro',uri=True); print(json.dumps(db.execute(sys.argv[1],json.loads(sys.argv[2])).fetchall()))"
        result = command(['docker', 'run', '--rm', '--network', 'none', '--user', f'{os.getuid()}:{os.getgid()}',
            '--cap-drop', 'ALL', '--volume', str(self.hosts[name]['home']) + ':/data', '--entrypoint', 'python3',
            'talos-e2e-firewall:local', '-c', source, query, json.dumps(values)])
        return [tuple(row) for row in json.loads(result)]

    def raw(self, name, path, body=None, method=None, content_type='application/json'):
        args = ['docker', 'exec', '-i', self.hosts[name]['container'], 'curl', '--config', '/home/talos/.talos/auth/request.conf',
                '--max-time', '30', '--header', 'Content-Type: ' + content_type]
        if method:
            args += ['--request', method]
        if body is not None:
            args += ['--data-binary', '@-']
        args += ['http://127.0.0.1:8080' + path]
        return subprocess.run(args, input=body, check=True, capture_output=True).stdout

    def api(self, name, path, body=None, method=None):
        return json.loads(self.raw(name, path, None if body is None else json.dumps(body).encode(), method))

    def healthy(self, name):
        try:
            return self.api(name, '/health')
        except subprocess.CalledProcessError:
            return False

    def turn(self, name, node, session, prompt):
        return data(self.api(name, '/v1/turns/start', {'nodeId': node, 'sessionId': session,
            'input': {'content': [{'type': 'text', 'text': prompt}]},
            'modelSelection': {'modelRef': {'provider': self.cluster.model['provider'], 'id': self.cluster.model['id']}, 'reasoningEffort': None},
            'userPermissionMode': 'fullAccess'}))

    def history(self, name, session, turn):
        reply = self.api(name, '/v1/sessions/' + session + '/history')
        if reply.get('error', {}).get('code') == 'resourceNotFound':
            return False
        history = data(reply)['data']
        result = next((t['result'] for t in history if t['identity']['turnId'] == turn), None)
        if result is None:
            return False
        require(result['status'] == 'completed', 'NAT task failed: ' + json.dumps(result))
        return history

    def isolated(self):
        for host in self.hosts.values():
            networks = json.loads(command(['docker', 'inspect', host['container']]))[0]['NetworkSettings']['Networks']
            require(len(networks) == 1, 'NAT fixture unexpectedly joined another network')
            host['address'] = next(iter(networks.values()))['IPAddress']
        for source, target in [('NAT-A', 'NAT-B'), ('NAT-B', 'NAT-A')]:
            result = subprocess.run(['docker', 'exec', self.hosts[source]['container'], 'curl', '--noproxy', '*',
                '--connect-timeout', '2', '--max-time', '3', f'http://{self.hosts[target]["address"]}:8080/health'], capture_output=True)
            require(result.returncode != 0, 'NAT ingress firewall allowed a direct peer connection')
        for host in self.hosts.values():
            ports = json.loads(command(['docker', 'inspect', host['container']]))[0]['NetworkSettings']['Ports']
            require(not any(ports.values()), 'NAT management host published an inbound port')

    def close(self):
        for host in self.hosts.values():
            with (REPORT / (host['container'] + '.log')).open('w') as log:
                subprocess.run(['docker', 'logs', host['container']], stdout=log, stderr=subprocess.STDOUT)
            subprocess.run(['docker', 'rm', '--force', host['container']], capture_output=True)
            self.cluster.containers.remove(host['container'])
        for network in self.networks:
            subprocess.run(['docker', 'network', 'rm', network], capture_output=True)


def run(cluster):
    hosts = NatHosts(cluster)
    try:
        hosts.start('NAT-A')
        hosts.start('NAT-B')
        hosts.isolated()
        node = cluster.nodes['Docker-Worker']['node_id']
        session = str(uuid.uuid4())
        marker = 'NAT_' + uuid.uuid4().hex
        filename = 'nat-proof-' + uuid.uuid4().hex + '.txt'
        first = hosts.turn('NAT-A', node, session, f'Use Bash to write exactly {marker} into {filename}. Then reply {marker}.')
        wait_for(lambda: hosts.history('NAT-A', session, first['turnId']), 'NAT-A task completed', 120)
        wait_for(lambda: hosts.history('NAT-B', session, first['turnId']), 'NAT-B imported original facts', 90)
        # Origin management API is completely stopped; the bound pure Worker remains alive.
        command(['docker', 'stop', '--time', '10', hosts.hosts['NAT-A']['container']])
        second = hosts.turn('NAT-B', node, session, f'Use Bash to read {filename}. Reply with its contents and NAT_CURRENT_COORDINATOR_OK.')
        history = wait_for(lambda: hosts.history('NAT-B', session, second['turnId']), 'NAT-B continued without origin management service', 120)
        require(marker in json.dumps(history[-1]['result']), 'NAT continuation lost remote Worker file')
        command(['docker', 'start', hosts.hosts['NAT-A']['container']])
        wait_for(lambda: hosts.healthy('NAT-A'), 'NAT origin original API restarted')
        hosts.firewall('NAT-A')
        hosts.isolated()
        wait_for(lambda: data(hosts.api('NAT-A', '/v1/sync/status'))['connection'] == 'connected', 'NAT origin verified account rebound')
        wait_for(lambda: hosts.history('NAT-A', session, second['turnId']) == history, 'NAT origin restart caught up', 90)
        data(hosts.api('NAT-A', '/v1/models?nodeId=' + node))
        from cluster import ROOT
        body = (ROOT / 'build/desktop/icons/icon.png').read_bytes()
        boundary = 'talos-' + uuid.uuid4().hex
        metadata = json.dumps({'filename': 'nat-source.png', 'scope': {'type': 'personal'}, 'tags': [], 'metadata': {}}).encode()
        multipart = (f'--{boundary}\r\nContent-Disposition: form-data; name="metadata"\r\nContent-Type: application/json\r\n\r\n'.encode()
            + metadata + f'\r\n--{boundary}\r\nContent-Disposition: form-data; name="file"; filename="nat-source.png"\r\nContent-Type: image/png\r\n\r\n'.encode()
            + body + f'\r\n--{boundary}--\r\n'.encode())
        file = data(json.loads(hosts.raw('NAT-A', '/v1/files', multipart, 'POST', 'multipart/form-data; boundary=' + boundary)))['file']
        path = '/v1/files/' + file['fileId']
        wait_for(lambda: hosts.api('NAT-B', path).get('status') == 'success', 'NAT source metadata imported')
        content = data(hosts.api('NAT-B', path + '/content'))
        require(base64.b64decode(content['dataBase64']) == body, 'NAT source FileService bytes changed')
        data(hosts.api('NAT-B', path, {'tags': ['nat-update'], 'metadata': None}, 'PATCH'))
        require(data(hosts.api('NAT-A', path))['file']['tags'] == ['nat-update'], 'NAT callback did not update source')
        data(hosts.api('NAT-B', path, method='DELETE'))
        require(hosts.api('NAT-A', path).get('error', {}).get('code') == 'resourceNotFound', 'NAT source delete was not authoritative')
        memory_marker = 'NAT_MEMORY_' + uuid.uuid4().hex
        remembered = hosts.turn('NAT-B', node, session, f'For all future collaboration, remember my permanent required report identifier is {memory_marker}. This is a lasting preference. Do not call tools, just acknowledge.')
        memory_history = wait_for(lambda: hosts.history('NAT-B', session, remembered['turnId']), 'NAT memory reflection completed', 120)
        worker_home = cluster.nodes['Docker-Worker']['home']
        require(any(memory_marker in path.read_text() for path in worker_home.rglob('MEMORY.md')), 'NAT reflection did not write memory on the Worker')
        wait_for(lambda: hosts.database('NAT-B', 'SELECT memory_processed_until_event_seq FROM sessions WHERE session_key=?', (session,))[0][0] > 0,
                 'NAT Dream Memory returned and committed its progress to current coordinator', 90)
        usage = set(hosts.database('NAT-B', 'SELECT DISTINCT usage_kind FROM profile_turn_usage WHERE session_id=?', (session,)))
        require({('memory',), ('dream',)} <= usage, 'NAT memory model callbacks did not return to the current coordinator')
        for name in ('NAT-A', 'NAT-B'):
            require(not list(hosts.hosts[name]['home'].rglob('MEMORY.md')), 'NAT control plane wrote remote Worker memory files')
        wait_for(lambda: hosts.history('NAT-A', session, remembered['turnId']) == memory_history, 'NAT memory Turn facts synchronized')
        child_marker = 'NAT_CHILD_' + uuid.uuid4().hex
        child_file = 'nat-child-' + uuid.uuid4().hex + '.txt'
        child_session = str(uuid.uuid4())
        child = hosts.turn('NAT-B', node, child_session, f'Use spawn_agent exactly once with child named writer. Child task: use Bash to sleep 8 seconds then write {child_marker} to {child_file} and reply {child_marker}. Immediately call wait_agent target writer timeoutMs 60000. After successful completion reply with the child result. Do not do the child task yourself.')
        child_history = wait_for(lambda: hosts.history('NAT-B', child_session, child['turnId']), 'NAT child callback and wait terminal completed', 150)
        children = hosts.database('NAT-B', 'SELECT session_key, execution_node_id FROM sessions WHERE parent_session_id=?', (child_session,))
        require(len(children) == 1 and children[0][1] == node, 'NAT child was not coordinated and persisted by B on the same Computer')
        from interactions import require_successful_wait
        require_successful_wait(child_history, child_marker, children[0][0])
        require(len(list(worker_home.rglob(child_file))) == 1, 'NAT child file missing from the bound Worker')
        wait_for(lambda: hosts.history('NAT-A', child_session, child['turnId']) == child_history, 'NAT child history synchronized to another control plane')
        hosts.isolated()
        print('CASE sync_nat_memory_and_child_callbacks_return_to_current_coordinator PASS', flush=True)
        (REPORT / 'nat-history.json').write_text(json.dumps(history, indent=2))
        print('CASE sync_two_nat_control_planes_continue_without_origin_and_source_files_use_gateway PASS', flush=True)
    finally:
        hosts.close()
