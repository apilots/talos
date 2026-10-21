"""Real Auth and owned-Computer cache restoration; no model completion required."""
import json
import os
from pathlib import Path
import subprocess
import sys

import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'multi_node'))
from fixture import Cluster, ROOT, REPORT, data, http, require, wait_for
from registration import Desktop


def proof(case):
    print(f'CASE {case} PASS', flush=True)


def run(cluster):
    cluster.start_control_plane()
    token = cluster.users['alice']['access_token']
    require(http(cluster.cloud_url + '/v1/auth/introspect', {'token': token})['active'], 'Real token was not active')
    require(http(cluster.url + '/health')['cloud_auth_url'] == cluster.cloud_url, 'Health advertised another Auth service')
    for candidate in (None, 'invalid-token'):
        denied = http(cluster.url + '/v1/user', token=candidate)
        require(denied.get('error', {}).get('code') == 'unauthenticated', 'User API accepted missing/invalid credentials')
    user = data(http(cluster.url + '/v1/user', token=token))['user']
    require(user['user_id'] == cluster.users['alice']['user']['user_id'], 'User API changed verified identity')
    require(user['role'] == cluster.users['alice']['user']['role'], 'User API changed verified role')
    proof('cloud_token_and_gateway_identity')

    home = cluster.root / 'cached-host'
    env = cluster.config(home, {})
    host = Desktop(cluster, home, 'auth-cache-host')
    try:
        host.login('alice')
        node_id = host.call('desktop/workerIdentity')['nodeId']
        wait_for(lambda: cluster.manage('alice', 'GetNode', {'node_id': node_id}).get('node', {}).get('available'),
                 'Owned host registration')
    finally:
        host.close()
    config_file = home / 'conf/talos.yaml'
    runner = yaml.safe_load(config_file.read_text())['agent_node']['runner']
    credential_file = Path(runner['credential_file'])
    credential = credential_file.read_bytes()
    cache = json.loads((home / 'auth/cloud-session.json').read_text())
    require(cache['user']['user_id'] == user['user_id'], 'Actual device login cached another account')
    require((home / 'auth/cloud-session.json').stat().st_mode & 0o777 == 0o600, 'Session cache permissions')
    proof('device_login_saved_owned_computer')

    def command(name, argv):
        with (REPORT / f'{name}.log').open('wb') as diagnostic, (REPORT / f'{name}.stdout').open('wb') as output:
            return subprocess.run(argv, executable=argv[0], env=env, stdout=output, stderr=diagnostic,
                                  close_fds=False, timeout=45)

    env['TALOS_APP_SERVER_BIN'] = os.environ.get('TALOS_E2E_APP_SERVER_BIN', str(ROOT / 'talos-rs/target/debug/app-server'))
    app = command('app-server-cache', [os.environ.get('TALOS_E2E_APP_SERVER_TEST_CLIENT_BIN', str(ROOT / 'talos-rs/target/debug/talos-app-server-test-client')), 'cloud-session-cache'])
    require(app.returncode == 0, 'Cached app-server authorization failed; inspect app-server-cache.log')
    proof('app_server_existing_owned_session')
    cli = [os.environ.get('TALOS_E2E_CLI_BIN', str(ROOT / 'talos-rs/target/debug/talos')), 'models', '--format', 'json']
    result = command('cli-cache', cli)
    require(result.returncode == 0, 'Cached CLI authorization failed; inspect cli-cache.log')
    models = json.loads((REPORT / 'cli-cache.stdout').read_text())
    require(isinstance(models, list) and models, 'CLI did not read the owned Worker model catalog')
    saved = yaml.safe_load(config_file.read_text())['agent_node']['runner']
    require(saved['node_id'] == node_id and credential_file.read_bytes() == credential, 'Cache restoration replaced Worker identity')
    owned = data(http(cluster.url + '/v1/computers', token=token))['data']
    require([node['nodeId'] for node in owned] == [node_id], 'Cache restoration created duplicate Computers')
    proof('cli_existing_owned_session_preserves_identity')

    cloud = next(process for process in cluster.processes
                 if Path(process.args[0]) == Path(os.environ['TALOS_CLOUD_SERVER_BIN']))
    cluster.stop(cloud)
    offline = command('cli-cache-offline', cli)
    require(offline.returncode != 0, 'CLI authorized from cache while Auth was offline')
    require(credential_file.read_bytes() == credential, 'Offline validation changed Worker credentials')
    proof('cli_cached_session_requires_cloud_validation')
    for path in REPORT.iterdir():
        if path.suffix not in ('.log', '.stdout'):
            continue
        contents = path.read_text(errors='replace')
        require(cache['access_token'] not in contents and credential.decode().strip() not in contents,
                f'Credential leaked into {path.name}')
    proof('cloud_gateway_auth')


if __name__ == '__main__':
    cluster = Cluster()
    try:
        run(cluster)
    finally:
        cluster.close()
