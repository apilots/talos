"""Observable multi-control-plane behavior through original product APIs."""
import json
import os
import sys
import subprocess
import time
import uuid
import yaml
from pathlib import Path
from cluster import REPORT, ROOT, Desktop, data, http, require, wait_for
from turn_helpers import submit, settled


def directory(cluster):
    expected = {d['node'] for d in cluster.desktops.values()} | {cluster.nodes[n]['node_id'] for n in ('Mac-Worker', 'Docker-Worker')}
    for name in cluster.desktops:
        wait_for(lambda name=name: {n['nodeId'] for n in cluster.desktop(name, 'computer/list', {'limit': 20})['data']} == expected,
                 name + ' complete same-account directory')
        require(cluster.desktop(name, 'sync/status')['localComputerId'] == cluster.desktops[name]['node'], 'Host-relative local identity is wrong')
    for name, os_type in [('Mac-Worker', 'macOs'), ('Docker-Worker', 'linux')]:
        node = next(n for n in cluster.list_nodes('alice') if n['nodeId'] == cluster.nodes[name]['node_id'])
        report = node['systemInfo']
        require(report and report['os'] == os_type and report['memoryBytes'] > 0 and report['logicalCpuCount'] > 0, name + ' hardware report missing')
        require(report['workerVersion'] and report['architecture'] and report['osVersion'], name + ' report is incomplete')
        if os_type == 'macOs':
            require(report['cpuModel'], 'Native Mac CPU model was not collected')
        for desktop in cluster.desktops.values():
            wait_for(lambda desktop=desktop: bool(cluster.database(desktop['home'], 'SELECT system_info FROM agent_nodes WHERE node_id = ?', (node['nodeId'],))), 'Hardware metadata materialized')
            persisted = json.loads(cluster.database(desktop['home'], 'SELECT system_info FROM agent_nodes WHERE node_id = ?', (node['nodeId'],))[0][0])
            require(persisted == report, 'Hardware metadata changed between control planes')
        require(not list(cluster.nodes[name]['home'].rglob('talos.db')), 'Pure Worker created a management business DB')
        (REPORT / (name + '-hardware.json')).write_text(json.dumps(report, indent=2))
    # The current account's private central-host Worker must never be injected into another user's list.
    require({n['nodeId'] for n in cluster.list_nodes('alice')} == expected, 'G Computer directory leaked another account or its own private Worker')
    require({n['nodeId'] for n in cluster.list_nodes('bob')} == {cluster.nodes['Bob-Worker']['node_id']}, 'Bob saw Alice Computers')
    require(http(cluster.url + '/v1/models?nodeId=embedded', token=cluster.users['alice']['access_token']).get('error', {}).get('code') == 'permissionDenied',
            'An ordinary account accessed the center private embedded Worker')
    require(http(cluster.url + '/v1/computers/embedded', token=cluster.users['alice']['access_token']).get('error', {}).get('code') == 'permissionDenied',
            'An ordinary account read the center private embedded Computer')
    print('CASE sync_directory_hardware_and_account_ownership PASS', flush=True)



def execution(cluster):
    for name in ('Desktop-A', 'Mac-Worker', 'Docker-Worker'):
        node = cluster.desktops[name]['node'] if name in cluster.desktops else cluster.nodes[name]['node_id']
        home = cluster.desktops[name]['home'] if name in cluster.desktops else cluster.nodes[name]['home']
        session = str(uuid.uuid4())
        filename = 'sync-proof-' + uuid.uuid4().hex + '.txt'
        marker = 'COMPUTER_SYNC_' + uuid.uuid4().hex
        # The original filesystem API creates the directory on the selected
        # Worker, including when its absolute path exists only in a container.
        parent = cluster.desktop('Desktop-A', 'fs/directory/read', {'nodeId': node})['directory']['path']
        project_directory = cluster.desktop('Desktop-A', 'fs/directory/create', {'nodeId': node, 'parent': parent, 'name': 'sync-project-' + uuid.uuid4().hex})['path']
        project = cluster.desktop('Desktop-A', 'project/create', {'nodeId': node, 'name': 'Shared Project ' + name, 'directory': project_directory})['project']
        for host in ('Desktop-B',):
            wait_for(lambda host=host: any(p['project_id'] == project['project_id'] and p['nodeId'] == node for p in cluster.desktop(host, 'project/list', {'nodeId': node})['projects']), 'Empty Project synchronized before first Session')
        first = submit(cluster, 'Desktop-A', node, session, f'Use Bash to write exactly {marker} to {filename} in the current workspace. Then reply exactly {marker}.', projectId=project['project_id'])
        wait_for(lambda: settled(cluster, 'Desktop-A', session, first['turnId']), 'First real Worker task settled', seconds=120)
        for host in ('Desktop-B', 'G'):
            wait_for(lambda host=host: bool(cluster.history(host, session)['data']), host + ' imported Session history')
        # A second coordinator must run on the same bound Computer, with old Worker files and source callbacks.
        second = submit(cluster, 'Desktop-B', node, session, f'Use Bash to sleep 10 seconds, then read {filename}. Reply with the contents and append CONTINUED_ON_SAME_COMPUTER.')
        wait_for(lambda: cluster.desktop('Desktop-A', 'session/active_turn', {'sessionId': session})['activeTurn'], 'Other Desktop received live Turn state')
        active = cluster.desktop('Desktop-A', 'session/active_turn', {'sessionId': session})['activeTurn']
        require(active['identity']['turnId'] == second['turnId'], 'Realtime state belongs to the wrong Turn')
        history = wait_for(lambda: settled(cluster, 'Desktop-B', session, second['turnId']), 'Remote coordinator task settled', seconds=120)
        for host in ('Desktop-A', 'G'):
            wait_for(lambda host=host: cluster.history(host, session)['data'] == history, host + ' complete identical history')
        updated_name = 'Renamed Shared Project ' + name
        cluster.desktop('Desktop-B', 'project/patch', {'project_id': project['project_id'], 'name': updated_name})
        wait_for(lambda: any(p['project_id'] == project['project_id'] and p['name'] == updated_name and p['session_count'] == 1
            for p in cluster.desktop('Desktop-A', 'project/list', {'nodeId': node})['projects']), 'Project count and patch synchronized')
        matches = list(home.rglob(filename))
        require(len(matches) == 1 and matches[0].read_text().strip() == marker, 'Worker file missing or wrong')
        for other in [cluster.server_home] + [d['home'] for d in cluster.desktops.values()] + [n['home'] for n in cluster.nodes.values()]:
            if other != home:
                require(not list(other.rglob(filename)), 'File was written outside the bound Worker')
        identity = cluster.desktop('Desktop-B', 'session/get', {'sessionId': session})
        require(identity['session']['nodeId'] == node and identity['session']['projectId'] == project['project_id'], 'Session migrated Computer or Project')
        cluster.desktop('Desktop-B', 'project/patch', {'project_id': project['project_id'], 'name': 'Continued Project ' + name})
        wait_for(lambda: cluster.desktop('Desktop-A', 'project/read', {'project_id': project['project_id']})['project']['name'] == 'Continued Project ' + name, 'Project rename synchronized back to source')
        (REPORT / (name + '-history.json')).write_text(json.dumps(history, indent=2))
        print('CASE sync_real_execution_' + name.replace('-', '_').lower() + ' PASS', flush=True)


def recovery(cluster):
    # A fresh management host cannot discover H. Its account opt-out must still
    # persist without a fake binding, and survive a real Desktop restart.
    subprocess.run(['docker', 'stop', '--time', '10', cluster.hub_container], check=True, capture_output=True)
    cold_home = cluster.root / 'Desktop-Cold'
    cluster.config(cold_home, {})
    cold_path = cold_home / 'conf/talos.yaml'
    document = yaml.safe_load(cold_path.read_text())
    document['cloud'].update(agent_base_url=f'https://127.0.0.1:{cluster.rpc_port}', agent_trusted_ca_file=str(cluster.cert))
    document['desktop'] = {'connection_mode': 'cloud'}
    document['sync'] = cluster.sync_config()
    cold_path.write_text(yaml.safe_dump(document, sort_keys=False))
    cold = Desktop(cluster, cold_home, 'Desktop-Cold')
    token = cold.login('alice')['access_token']
    offline = cluster.call(cold, token, 'sync/status')
    require(offline['connection'] == 'disconnected', 'Initial Sync outage was not visible')
    require(not cluster.database(cold_home, 'SELECT account_id FROM sync_bindings'), 'Failed discovery created a fake binding')
    cold_config = cold_path.read_bytes()
    disabled = cluster.call(cold, token, 'sync/disable')
    require(disabled['configuredEnabled'] is False and disabled['connection'] == 'disabled', 'Initial account opt-out was not saved')
    require(cold_path.read_bytes() == cold_config, 'Unbound opt-out changed deployment YAML')
    cold.close()
    cold = Desktop(cluster, cold_home, 'Desktop-Cold')
    token = cold.call('auth/session/read', {})['session']['access_token']
    require(cluster.call(cold, token, 'sync/status')['configuredEnabled'] is False, 'Unbound opt-out was lost on restart')
    subprocess.run(['docker', 'start', cluster.hub_container], check=True, capture_output=True)
    wait_for(lambda: cluster.desktop('Desktop-B', 'sync/status')['connection'] == 'connected', 'Other host recovered its enabled account')
    disabled = cluster.call(cold, token, 'sync/status')
    require(disabled['connection'] == 'disabled', 'H recovery automatically enabled an opted-out account')
    require(not cluster.database(cold_home, 'SELECT account_id FROM sync_bindings'), 'Opted-out status discovered a binding')
    cluster.call(cold, token, 'sync/enable', {'claimLocalHistory': False})
    wait_for(lambda: cluster.call(cold, token, 'sync/status')['connection'] == 'connected', 'Explicit enable after initial outage')
    require(cold_path.read_bytes() == cold_config, 'Unbound re-enable changed deployment YAML')
    cold.close()
    print('CASE sync_first_outage_opt_out_restart_and_explicit_enable PASS', flush=True)

    name = 'Desktop-A'
    desktop = cluster.desktops[name]
    local_id = desktop['node']
    before_config = (desktop['home'] / 'conf/talos.yaml').read_bytes()
    # All accounts authenticate through the actual Cloud service. The product
    # switch belongs to one host/account even when that account is an admin.
    def g_status(user):
        return data(http(cluster.url + '/v1/sync/status', token=cluster.users[user]['access_token']))
    for user in ('alice', 'bob', 'admin'):
        wait_for(lambda user=user: g_status(user)['connection'] == 'connected', user + ' center sync connected')
    before_g = Path(cluster.server_env['TALOS_CONFIG']).read_bytes()
    require(data(http(cluster.url + '/v1/sync/disable', {}, cluster.users['admin']['access_token']))['configuredEnabled'] is False,
            'Administrator account opt-out was not saved')
    for user in ('alice', 'bob'):
        require(g_status(user)['connection'] == 'connected', 'Administrator opt-out stopped another account')
    require(Path(cluster.server_env['TALOS_CONFIG']).read_bytes() == before_g, 'Account opt-out changed host YAML')
    data(http(cluster.url + '/v1/sync/enable', {'claimLocalHistory': False}, cluster.users['admin']['access_token']))
    wait_for(lambda: g_status('admin')['connection'] == 'connected', 'Administrator account sync resumed')
    # Deployment policy disables H for every account on this management host.
    # It must not overwrite their individual choices or another host's settings.
    deployment_path = Path(cluster.server_env['TALOS_CONFIG'])
    deployment = yaml.safe_load(before_g)
    deployment['sync']['enabled'] = False
    deployment_path.write_text(yaml.safe_dump(deployment, sort_keys=False))
    try:
        for user in ('alice', 'bob', 'admin'):
            require(g_status(user)['connection'] == 'disabled', 'Deployment opt-out left an account connected')
            denied = http(cluster.url + '/v1/sync/enable', {'claimLocalHistory': False}, cluster.users[user]['access_token'])
            require(denied.get('error', {}).get('code') == 'permissionDenied', 'Account enable overrode deployment policy')
        require(cluster.desktop('Desktop-B', 'sync/status')['connection'] == 'connected', 'Deployment policy stopped another management host')
    finally:
        deployment_path.write_bytes(before_g)
    for user in ('alice', 'bob', 'admin'):
        wait_for(lambda user=user: g_status(user)['connection'] == 'connected', user + ' resumed after deployment policy restored')
    print('CASE sync_deployment_policy_overrides_all_local_accounts PASS', flush=True)
    disabled = cluster.desktop(name, 'sync/disable')
    require(disabled['configuredEnabled'] is False, 'Sync opt-out was not saved')
    require((desktop['home'] / 'conf/talos.yaml').read_bytes() == before_config, 'Desktop opt-out changed host YAML')
    require(cluster.desktop('Desktop-B', 'sync/status')['connection'] == 'connected', 'Opt-out stopped the same account on another host')
    require(g_status('alice')['connection'] == 'connected', 'Desktop opt-out stopped the center account')

    session = str(uuid.uuid4())
    ack = submit(cluster, name, local_id, session, 'Reply exactly LOCAL_WITH_SYNC_DISABLED.')
    wait_for(lambda: settled(cluster, name, session, ack['turnId']), 'Local execution with sync off', seconds=120)
    require(cluster.desktop(name, 'sync/status')['pendingChanges'] > 0, 'Offline facts were not captured')
    require(http(cluster.url + '/v1/sessions/' + session, token=cluster.users['alice']['access_token'])['status'] != 'success', 'Disabled changes were published')
    # Real Desktop controller/app-server restart reads the saved account and Worker identity.
    desktop['host'].close()
    desktop['host'] = Desktop(cluster, desktop['home'], name)
    cache = desktop['host'].call('auth/session/read', {})['session']
    require(cache['user']['user_id'] == cluster.users['alice']['user']['user_id'], 'Saved verified account was lost')
    desktop['token'] = cache['access_token']
    status = cluster.desktop(name, 'sync/status')
    require(status['localComputerId'] == local_id and status['configuredEnabled'] is False, 'Restart changed UUID or opt-out')
    cluster.desktop(name, 'sync/enable', {'claimLocalHistory': False})
    wait_for(lambda: bool(cluster.history('Desktop-B', session)['data']), 'Restarted outbox caught up')
    require((desktop['home'] / 'conf/talos.yaml').read_bytes() == before_config, 'Re-enable changed host YAML')
    print('CASE sync_desktop_restart_opt_out_and_outbox PASS', flush=True)
    # H is restarted as a real container using the same database and volume.
    subprocess.run(['docker', 'stop', '--time', '10', cluster.hub_container], check=True, capture_output=True)
    ack = submit(cluster, name, local_id, session, 'Reply exactly LOCAL_DURING_H_OUTAGE.')
    wait_for(lambda: settled(cluster, name, session, ack['turnId']), 'Local task during H outage', seconds=120)
    subprocess.run(['docker', 'start', cluster.hub_container], check=True, capture_output=True)
    wait_for(lambda: settled(cluster, 'Desktop-B', session, ack['turnId']), 'Hub durable recovery and delta catch-up', seconds=90)
    print('CASE sync_hub_container_restart_and_local_execution PASS', flush=True)
    # Pure Mac and Linux Workers retain their registered identity through process/container restart.
    for name in ('Mac-Worker', 'Docker-Worker'):
        node = cluster.nodes[name]
        previous = cluster.info(name)['node_id']
        cluster.manage('alice', 'RenameComputer', {'node_id': previous, 'display_name': name + ' renamed'})
        if 'container' in node:
            subprocess.run(['docker', 'restart', node['container']], check=True, capture_output=True)
        else:
            cluster.stop(node['process'])
            node['process'] = cluster.start_process(name + '-restart', [str(ROOT / 'talos-rs/target/debug/talos-worker')],
                dict(os.environ, HOME=str(node['home']), TALOS_HOME=str(node['home']), TALOS_CONFIG=str(node['home'] / 'conf/talos.yaml'), TALOS_RESOURCE_DIR=str(ROOT)))
        wait_for(lambda name=name: cluster.info(name).get('available'), 'Renamed Worker recovered')
        require(cluster.info(name)['node_id'] == previous, 'Restart or rename changed Worker UUID')
    print('CASE sync_mac_docker_worker_restart_keeps_uuid PASS', flush=True)


def isolation(cluster):
    node = cluster.nodes['Bob-Worker']['node_id']
    session = str(uuid.uuid4())
    response = http(cluster.url + '/v1/turns/start', {'nodeId': node, 'sessionId': session,
                    'input': {'content': [{'type': 'text', 'text': 'Reply exactly BOB_ONLY_HISTORY.'}]},
                    'modelSelection': {'modelRef': {'provider': cluster.model['provider'], 'id': cluster.model['id']}, 'reasoningEffort': None},
                    'userPermissionMode': 'fullAccess'}, cluster.users['bob']['access_token'])
    data(response)
    def bob_completed():
        history = data(http(cluster.url + '/v1/sessions/' + session + '/history', token=cluster.users['bob']['access_token']))['data']
        if not history:
            return False
        require(history[-1]['result']['status'] == 'completed', 'Bob task failed')
        return history
    wait_for(bob_completed, 'Bob task settled', seconds=120)
    denied = http(cluster.url + '/v1/sessions/' + session, token=cluster.users['alice']['access_token'])
    require(denied['status'] != 'success', 'Alice read Bob history')
    for desktop in cluster.desktops.values():
        require(not cluster.database(desktop['home'], 'SELECT id FROM sessions WHERE session_key = ?', (session,)), 'Another account history reached Desktop DB')
    require(cluster.query('SELECT id FROM sessions WHERE session_key = ?', (session,)), 'Center failed to retain Bob history')
    print('CASE sync_center_complete_desktop_current_account_only PASS', flush=True)


if __name__ == '__main__':
    from cluster import SyncCluster
    cluster = SyncCluster()
    try:
        cluster.start()
        directory(cluster)
        if sys.argv[1] == 'execution':
            if os.environ.get('TALOS_E2E_REVIEW_UI') == '1':
                cluster.start_review_ui()
            execution(cluster)
        elif sys.argv[1] == 'recovery':
            recovery(cluster)
            isolation(cluster)
        elif sys.argv[1] == 'interactions':
            from interactions import source_files, commands, question_and_child, waiting_plugin_mutations_with_sync_offline
            source_files(cluster)
            commands(cluster)
            question_and_child(cluster)
            waiting_plugin_mutations_with_sync_offline(cluster)
        elif sys.argv[1] == 'faults':
            from faults import clouds, local_history_cursor
            clouds(cluster)
            local_history_cursor(cluster)
        elif sys.argv[1] == 'owner-configuration':
            from owner_configuration import run
            run(cluster)
        elif sys.argv[1] == 'nat':
            from nat import run
            run(cluster)
    finally:
        cluster.close()
