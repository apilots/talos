"""Real device login, Desktop controller, TLS ingress and durable node lifecycle."""

import json
import os
from pathlib import Path
import selectors
import ssl
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'multi_node'))
from fixture import Cluster, REPORT, ROOT, data, free_port, http, require, wait_for


class Desktop:
    def __init__(self, cluster, home, name):
        self.cluster, self.home, self.name = cluster, home, name
        self.sequence = 0
        self.login_tokens = []
        self.log = (REPORT / f'{name}.log').open('ab')
        env = dict(os.environ, HOME=str(home), TALOS_HOME=str(home), TALOS_CONFIG=str(home / 'conf/talos.yaml'),
                   TALOS_RESOURCE_DIR=str(ROOT), TALOS_DESKTOP_HOSTNAME=name,
                   TALOS_APP_SERVER_BIN=str(ROOT / 'talos-rs/target/debug/app-server'))
        self.process = subprocess.Popen(['node', str(REPORT / 'desktop-host.mjs')], env=env,
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.process.stdout, selectors.EVENT_READ)
        self.pending = b''
        cluster.processes.append(self.process)
        self.call('connect')

    def request(self, method, params=None, token=None):
        self.sequence += 1
        self.process.stdin.write((json.dumps({'id': self.sequence, 'method': method, 'params': params, 'actorToken': token}) + '\n').encode())
        self.process.stdin.flush()
        deadline = time.monotonic() + 45
        while time.monotonic() < deadline:
            if b'\n' not in self.pending:
                if not self.selector.select(timeout=0.25):
                    require(self.process.poll() is None, f'{self.name}: Desktop exited before {method}')
                    continue
                chunk = os.read(self.process.stdout.fileno(), 65536)
                require(chunk, f'{self.name}: Desktop EOF before {method}')
                self.pending += chunk
                continue
            line, self.pending = self.pending.split(b'\n', 1)
            response = json.loads(line)
            require(response['id'] == self.sequence, f'{self.name}: response correlation failed')
            return response
        raise AssertionError(f'{self.name}: {method} timed out')

    def call(self, method, params=None):
        response = self.request(method, params)
        require('result' in response, f'{self.name}: {method} failed code={response.get("error", {}).get("code")}')
        return response['result']

    def login(self, user):
        started = self.call('auth/login/start', {'client_name': self.name, 'client_kind': 'desktop'})
        approved = http(self.cluster.cloud_url + '/v1/auth/device/approve',
                        {'session_id': started['login_id'], 'user_code': started['user_code']},
                        self.cluster.users[user]['access_token'])
        require(approved.get('status') == 'approved', 'Real device login approval failed')
        result = self.call('auth/login/status', {'login_id': started['login_id']})
        require(result['status'] == 'authenticated', 'Device login did not authenticate')
        self.login_tokens.append(result['access_token'])
        cache = json.loads((self.home / 'auth/cloud-session.json').read_text())
        require(cache['access_token'] == result['access_token'], 'Actual login was not saved')
        require(cache['user']['user_id'] == self.cluster.users[user]['user']['user_id'], 'Saved login owner mismatch')
        require((self.home / 'auth/cloud-session.json').stat().st_mode & 0o777 == 0o600, 'Login cache permissions')
        return result

    def close(self):
        if self.process.poll() is None:
            self.process.stdin.close()
            require(self.process.wait(timeout=45) == 0, f'{self.name}: Desktop did not drain cleanly')
        self.selector.close()
        self.log.close()
        diagnostics = self.home / 'logs/talos.log'
        if diagnostics.exists():
            (REPORT / f'{self.name}-app-server.log').write_bytes(diagnostics.read_bytes())


def public(cluster, path, user='alice', method='GET', body=None):
    request = urllib.request.Request(cluster.ingress + path, method=method,
                                    data=None if body is None else json.dumps(body).encode(),
                                    headers={'Content-Type': 'application/json',
                                             'Authorization': 'Bearer ' + cluster.users[user]['access_token']})
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}),
                                       urllib.request.HTTPSHandler(context=ssl.create_default_context(cafile=cluster.cert)))
    try:
        response = opener.open(request, timeout=15)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        return json.load(response)


def start_ingress(cluster):
    if not hasattr(cluster, 'ingress'):
        cluster.ingress = f'https://127.0.0.1:{free_port()}'
    config = cluster.root / 'Caddyfile'
    config.write_text(f'''{{
    admin off
    auto_https off
}}
{cluster.ingress} {{
    tls {cluster.cert} {cluster.key}
    @grpc path /talos.agent.v1.*
    handle @grpc {{
        reverse_proxy https://127.0.0.1:{cluster.rpc_port} {{
            transport http {{
                tls_trust_pool file {cluster.cert}
            }}
        }}
    }}
    handle {{
        reverse_proxy {cluster.url}
    }}
}}
''')
    cluster.start_process('ingress', [os.environ['TALOS_E2E_CADDY_BIN'], 'run', '--config', str(config)],
                          dict(os.environ, XDG_CONFIG_HOME=str(cluster.root / 'caddy-config'),
                               XDG_DATA_HOME=str(cluster.root / 'caddy-data')))
    wait_for(lambda: public(cluster, '/v1/computers').get('status') == 'success', 'TLS product ingress')


def configure_desktop(cluster, name):
    home = cluster.root / name
    cluster.config(home, {})
    path = home / 'conf/talos.yaml'
    import yaml
    config = yaml.safe_load(path.read_text())
    config['cloud'].update(agent_base_url=cluster.ingress, agent_trusted_ca_file=str(cluster.cert))
    path.write_text(yaml.safe_dump(config, sort_keys=False))
    return home


def info(cluster, node_id):
    return data(public(cluster, f'/v1/computers/{node_id}'))


def registered(cluster, node_id):
    def probe():
        node = info(cluster, node_id)
        require(not node['available'] or node['health'] == 'healthy', 'Available node has no healthy connection')
        return node if node['leaseExpiresAt'] and node['leaseExpiresAt'] > time.time() else None
    return wait_for(probe, 'Outbound registration')


def proof(case, node=None):
    print(f'CASE {case} PASS', flush=True)
    with (REPORT / 'registration-evidence.jsonl').open('a') as output:
        output.write(json.dumps({'case': case, 'node': node}) + '\n')


def run(cluster):
    cluster.start_control_plane()
    start_ingress(cluster)
    home = configure_desktop(cluster, 'desktop-a')
    first = Desktop(cluster, home, 'desktop-a')
    second = None
    try:
        require(first.call('desktop/workerIdentity') == {'nodeId': 'embedded', 'controlPlaneEndpoint': cluster.ingress},
                'Fresh Desktop must expose its unregistered identity and configured Server')
        first.login('alice')
        identity = first.call('desktop/workerIdentity')
        node_id = identity['nodeId']
        require(str(uuid.UUID(node_id)) == node_id, 'Control plane did not create UUID')
        node = registered(cluster, node_id)
        require(node['ownerUserId'] == cluster.users['alice']['user']['user_id'], 'Node owner mismatch')
        require(node['mode'] == 'remote' and node['endpoint'] is None, 'Desktop exposed a direct listener')
        import yaml
        config = yaml.safe_load((home / 'conf/talos.yaml').read_text())
        runner = config['agent_node']['runner']
        credential = Path(runner['credential_file'])
        require(runner['node_id'] == node_id and runner['control_plane_endpoint'] == cluster.ingress, 'Saved identity mismatch')
        require(credential.parent == home / 'conf' and credential.stat().st_mode & 0o777 == 0o600, 'Worker credential location/permissions')
        require((home / 'data/talos.db').exists(), 'Desktop management runtime did not create its product DB')
        secrets = [credential.read_text(), json.loads((home / 'auth/cloud-session.json').read_text())['access_token']]
        require(all(secret not in json.dumps(identity) for secret in secrets), 'Safe identity response leaked a credential')
        proof('desktop_real_device_login_saved_registration', node)

        other_nodes = data(public(cluster, '/v1/computers', user='bob'))['data']
        require(node_id not in [item['nodeId'] for item in other_nodes], 'Another account sees private node')
        require(public(cluster, f'/v1/computers/{node_id}', user='bob')['status'] == 'error', 'Another account reads private node')
        proof('computer_owner_isolation')

        initial_lease = cluster.query('SELECT lease_token_hash FROM agent_nodes WHERE node_id = ?', (node_id,))[0][0]
        rejected = cluster.rpc('AgentNodeControl', 'Heartbeat', {
            'auth': {'node_id': node_id, 'token': 'invalid-lease'}, 'params': {'health': 'NODE_HEALTH_HEALTHY'}})
        require(rejected.get('failure', {}).get('error', {}).get('code') == 'ERROR_CODE_UNAUTHENTICATED',
                f'Invalid lease rejection had wrong code: {rejected.get("failure", {}).get("error", {}).get("code")}')
        initial_heartbeat, initial_expiry = node['lastHeartbeatAt'] or 0, node['leaseExpiresAt']
        wait_for(lambda: (info(cluster, node_id)['lastHeartbeatAt'] or 0) > initial_heartbeat, 'Real heartbeat', seconds=25)
        renewed = wait_for(lambda: next((n for n in [info(cluster, node_id)] if n['leaseExpiresAt'] > initial_expiry), None),
                           'Real 30-second lease renewal', seconds=45)
        require(cluster.query('SELECT lease_token_hash FROM agent_nodes WHERE node_id = ?', (node_id,))[0][0] == initial_lease,
                'Lease extension was re-registration instead of RenewLease')
        proof('worker_real_heartbeat_and_renewal', renewed)

        data(public(cluster, f'/v1/computers/{node_id}/name', method='PATCH',
                    body={'nodeId': node_id, 'displayName': 'Renamed personal computer'}))
        first.close()
        require(info(cluster, node_id)['leaseExpiresAt'] is None, 'Desktop exit did not unregister lease')
        first = Desktop(cluster, home, 'desktop-a-renamed-host')
        require(first.call('desktop/workerIdentity') == identity, 'Restart changed identity or control plane')
        restarted = registered(cluster, node_id)
        require(restarted['displayName'] == 'Renamed personal computer', 'Restart overwrote display name')
        require(credential.read_text() == secrets[0], 'Normal restart rotated registration credential')
        require(len([n for n in data(public(cluster, '/v1/computers'))['data'] if n['mode'] == 'remote']) == 1,
                'Restart created a duplicate node')
        proof('desktop_restart_reuses_uuid_and_credential', restarted)

        old_lease = cluster.query('SELECT lease_token_hash FROM agent_nodes WHERE node_id = ?', (node_id,))[0][0]
        Cluster.stop(cluster.server)
        cluster.start_server()
        recovered = wait_for(lambda: next((n for n in [info(cluster, node_id)] if n['leaseExpiresAt'] is not None
                             and n['leaseExpiresAt'] > time.time() and n['health'] == 'healthy'), None),
                             'Control-plane restart re-registration', seconds=45)
        require(cluster.query('SELECT lease_token_hash FROM agent_nodes WHERE node_id = ?', (node_id,))[0][0] != old_lease,
                'Control-plane restart did not issue a fresh lease')
        proof('control_plane_restart_keeps_node', recovered)

        data(public(cluster, f'/v1/computers/{node_id}/status', method='PUT',
                    body={'nodeId': node_id, 'status': 'disabled'}))
        require(info(cluster, node_id)['leaseExpiresAt'] is None, 'Disable did not revoke lease')
        # Allow the real 10-second heartbeat to observe revocation before recovery.
        time.sleep(12)
        data(public(cluster, f'/v1/computers/{node_id}/status', method='PUT',
                    body={'nodeId': node_id, 'status': 'enabled'}))
        first.call('desktop/workerEnroll', {'nodeId': node_id})
        require(credential.read_text() != secrets[0], 'Explicit recovery did not rotate credential')
        secrets.append(credential.read_text())
        rejected = cluster.rpc('AgentNodeControl', 'Register', {
            'auth': {'node_id': node_id, 'token': secrets[0]}, 'params': {}})
        require(rejected.get('failure', {}).get('error', {}).get('code') == 'ERROR_CODE_UNAUTHENTICATED',
                'Rotated registration credential was accepted')
        proof('computer_disable_enable_credential_recovery', registered(cluster, node_id))

        home_b = configure_desktop(cluster, 'desktop-b')
        second = Desktop(cluster, home_b, 'desktop-b')
        second.login('alice')
        secrets.extend(second.login_tokens)
        second_id = second.call('desktop/workerIdentity')['nodeId']
        require(second_id != node_id, 'Distinct computer reused another computer UUID')
        registered(cluster, second_id)
        second_runner = yaml.safe_load((home_b / 'conf/talos.yaml').read_text())['agent_node']['runner']
        secrets.append(Path(second_runner['credential_file']).read_text())
        require({node_id, second_id} <= {n['nodeId'] for n in data(public(cluster, '/v1/computers'))['data']},
                'Same-account API did not list both computers')
        proof('same_account_multiple_computers')

        first.call('auth/session/clear', {})
        require(info(cluster, node_id)['leaseExpiresAt'] is None, 'Sign-out did not unregister')
        require(first.call('auth/session/read', {})['session'] is None, 'Sign-out retained login')
        first.login('bob')
        require(info(cluster, node_id)['leaseExpiresAt'] is None, 'Other account resumed old owner Worker')
        require('error' in first.request('desktop/workerEnroll', {'nodeId': None}), 'Other account reused stored identity')
        first.call('auth/session/clear', {})
        first.login('alice')
        proof('account_switch_rejects_saved_owner_identity', registered(cluster, node_id))
        secrets.extend(first.login_tokens)
        logout_token = first.login_tokens[-1]
        require('result' in first.request('auth/logout', {}, token=logout_token), 'Real logout failed')
        require(not http(cluster.cloud_url + '/v1/auth/introspect', {'token': logout_token})['active'],
                'Logout did not revoke actual login token')
        require(first.call('auth/session/read', {})['session'] is None, 'Logout retained cache')
        require(info(cluster, node_id)['leaseExpiresAt'] is None, 'Logout retained Worker lease')
        proof('real_logout_revokes_login_and_unregisters')
    finally:
        if second is not None:
            second.close()
        first.close()
    for path in REPORT.glob('*.log'):
        contents = path.read_text(errors='replace')
        require(all(secret not in contents for secret in secrets), f'Worker/login secret leaked into {path.name}')
    proof('registration_secrets_not_logged')


if __name__ == "__main__":
    cluster = Cluster()
    try:
        run(cluster)
    finally:
        cluster.close()
