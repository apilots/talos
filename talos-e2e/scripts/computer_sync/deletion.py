"""Registration deletion across real independent management hosts and WSS Sync."""
import json
import os
import subprocess
import uuid
import yaml

from cluster import Desktop, REPORT, ROOT, data, http, require, wait_for
from turn_helpers import submit, settled


def run(cluster):
    worker = cluster.nodes['Mac-Worker']
    node = worker['node_id']
    session = str(uuid.uuid4())
    filename = 'retained-' + uuid.uuid4().hex + '.txt'
    marker = 'DELETION_HISTORY_' + uuid.uuid4().hex
    parent = cluster.desktop('Desktop-A', 'fs/directory/read', {'nodeId': node})['directory']['path']
    directory = cluster.desktop('Desktop-A', 'fs/directory/create', {
        'nodeId': node, 'parent': parent, 'name': 'deletion-' + uuid.uuid4().hex})['path']
    project = cluster.desktop('Desktop-A', 'project/create', {
        'nodeId': node, 'name': 'Retained after registration deletion', 'directory': directory})['project']
    first = submit(cluster, 'Desktop-A', node, session,
        f'Use Bash to write exactly {marker} to {filename} in the current workspace. Then reply exactly {marker}.',
        projectId=project['project_id'])
    history = wait_for(lambda: settled(cluster, 'Desktop-A', session, first['turnId']),
                       'Real task settled before registration deletion', seconds=180)
    for host in ('Desktop-B', 'G'):
        wait_for(lambda host=host: cluster.history(host, session)['data'] == history,
                 host + ' identical pre-deletion history')
    files = list(worker['home'].rglob(filename))
    require(len(files) == 1 and files[0].read_text().strip() == marker, 'Real Worker proof file missing')
    old_credential = (worker['home'] / 'conf/node.credential').read_text().strip()
    print('CASE sync_deletion_real_project_session_and_worker_file PASS', flush=True)

    # Leave B with its real pre-deletion CRDT/database state, then remove through A.
    cluster.desktop('Desktop-B', 'sync/disable')
    b = cluster.desktops['Desktop-B']
    require(cluster.database(b['home'], 'SELECT node_id FROM agent_nodes WHERE node_id = ?', (node,)),
            'Stale management replica did not retain the original registration')
    cluster.stop(worker['process'])
    wait_for(lambda: not cluster.manage('alice', 'GetNode', {'node_id': node})['node'].get('available'),
             'Worker offline before owner deletion')
    removed = cluster.desktop('Desktop-A', 'computer/delete', {'nodeId': node})
    require(removed == {'nodeId': node}, 'Forwarded deletion returned another identity')

    def removed_from(home):
        tombstone = cluster.database(home, 'SELECT owner_user_id FROM computer_deletions WHERE node_id = ?', (node,))
        registered = cluster.database(home, 'SELECT node_id FROM agent_nodes WHERE node_id = ?', (node,))
        return tombstone == [(cluster.users['alice']['user']['user_id'],)] and not registered

    a = cluster.desktops['Desktop-A']
    wait_for(lambda: removed_from(cluster.server_home), 'Authoritative owned deletion')
    # Observe the database before calling computer/list: list must not reconcile
    # against G and accidentally stand in for actual Sync materialization.
    wait_for(lambda: removed_from(a['home']), 'Deletion imported by A over WSS')
    require(cluster.database(b['home'], 'SELECT node_id FROM agent_nodes WHERE node_id = ?', (node,)),
            'Disabled B unexpectedly lost its stale registration before Sync resumed')
    cluster.desktop('Desktop-B', 'sync/enable', {'claimLocalHistory': False})
    wait_for(lambda: removed_from(b['home']), 'Stale B converged through WSS deletion')
    print('CASE sync_deletion_offline_replica_catches_up_without_resurrection PASS', flush=True)

    def assert_retained(host):
        require(cluster.history(host, session)['data'] == history, host + ' lost or rewrote Session history')
        identity = cluster.desktop(host, 'session/get', {'sessionId': session})['session']
        require(identity['nodeId'] == node and identity['projectId'] == project['project_id'],
                host + ' reassigned an immutable historical binding')
        saved = cluster.desktop(host, 'project/read', {'project_id': project['project_id']})['project']
        require(saved['nodeId'] == node and saved['directory'] == directory,
                host + ' lost the retained Project')
        require(saved['status']['state'] == 'nodeUnavailable', host + ' retained Project claims executable access')
        projects = cluster.desktop(host, 'project/list', {'nodeId': node})['projects']
        require(any(p['project_id'] == project['project_id'] for p in projects),
                host + ' could not list Projects bound to the removed Computer')
        sessions = cluster.desktop(host, 'session/list', {'nodeId': node})
        require(any(s['id'] == session for s in sessions['sessions']),
                host + ' could not list Sessions bound to the removed Computer')
        require(node not in {n['nodeId'] for n in cluster.desktop(host, 'computer/list', {'limit': 100})['data']},
                host + ' exposed the removed execution Computer')

    for host in ('Desktop-A', 'Desktop-B'):
        assert_retained(host)
    require(cluster.history('G', session)['data'] == history, 'G lost history after registration deletion')
    require(files[0].read_text().strip() == marker, 'Deletion removed Workspace data')

    # Restart using exactly the original UUID/config/credential, never a fabricated
    # database mutation. Rejected registration cannot republish an old Computer.
    worker['process'] = cluster.start_process('Mac-Worker-deleted-restart',
        [str(ROOT / 'talos-rs/target/debug/talos-worker')],
        dict(os.environ, TALOS_HOME=str(worker['home']), TALOS_CONFIG=str(worker['home'] / 'conf/talos.yaml'),
             TALOS_RESOURCE_DIR=str(ROOT)))
    rejected = cluster.rpc('AgentNodeControl', 'Register', {
        'auth': {'node_id': node, 'token': old_credential},
        'params': {'deployments': [{'agent_id': 'default'}]}})
    require(rejected.get('failure', {}).get('error', {}).get('code') == 'ERROR_CODE_UNAUTHENTICATED',
            'Old registration credential was accepted after deletion')
    wait_for(lambda: worker['process'].poll() is not None, 'Old Worker rejected on actual startup')
    require(worker['process'].returncode != 0, 'Old Worker silently accepted deleted registration')
    print('CASE sync_deletion_saved_worker_restart_and_old_credentials_rejected PASS', flush=True)

    # Restart a management host with its original DB and saved account, then H.
    b['host'].close()
    b['host'] = Desktop(cluster, b['home'], 'Desktop-B')
    b['token'] = b['host'].call('auth/session/read', {})['session']['access_token']
    subprocess.run(['docker', 'restart', cluster.hub_container], check=True, capture_output=True)
    for host, desktop in cluster.desktops.items():
        wait_for(lambda host=host: cluster.desktop(host, 'sync/status')['connection'] == 'connected',
                 host + ' WSS reconnected after restart')
        require(removed_from(desktop['home']), host + ' resurrected registration after restart')
        assert_retained(host)
    require(removed_from(cluster.server_home), 'G resurrected the removed registration')
    require(cluster.history('G', session)['data'] == history, 'G lost history after hub restart')
    print('CASE sync_deletion_management_and_hub_restart_preserve_history PASS', flush=True)

    # A new database must import retained history even though its Computer is
    # already a tombstone. This proves dependency resolution on a late joiner.
    home = cluster.root / 'Desktop-C'
    cluster.config(home, {})
    path = home / 'conf/talos.yaml'
    config = yaml.safe_load(path.read_text())
    config['cloud'].update(agent_base_url=f'https://127.0.0.1:{cluster.rpc_port}', agent_trusted_ca_file=str(cluster.cert))
    config['sync'] = cluster.sync_config()
    path.write_text(yaml.safe_dump(config, sort_keys=False))
    cold = Desktop(cluster, home, 'Desktop-C')
    # Register cleanup ownership before login/assertions can fail.
    cluster.desktops['Desktop-C'] = {'host': cold, 'home': home, 'token': None, 'node': None}
    token = cold.login('alice')['access_token']
    cluster.desktops['Desktop-C'].update(token=token, node=cluster.call(cold, token, 'sync/status')['localComputerId'])
    wait_for(lambda: removed_from(home), 'Fresh management host imported tombstone')
    wait_for(lambda: cluster.history('Desktop-C', session)['data'] == history,
             'Fresh host imported historical Session after Computer deletion')
    assert_retained('Desktop-C')
    require(files[0].read_text().strip() == marker, 'Restart or late import removed Workspace data')
    (REPORT / 'retained-deletion-history.json').write_text(json.dumps(history, indent=2))
    (REPORT / 'deletion-summary.json').write_text(json.dumps({
        'removedComputerId': node, 'sessionId': session, 'projectId': project['project_id'],
        'managementHosts': ['G', *cluster.desktops], 'historyEntries': len(history),
        'staleReplicaCatchup': True, 'oldCredentialRejected': True, 'workerRestartRejected': True,
        'hubAndManagementRestart': True, 'freshHostHistoricalImport': True,
        'workspaceFileRetained': True}, indent=2))
    print('CASE sync_deletion_late_joiner_imports_history_without_old_computer PASS', flush=True)
