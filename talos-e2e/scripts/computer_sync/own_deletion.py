"""Own Desktop deletion must not depend on page polling or rebind later work."""
import json
import uuid
from cluster import Desktop, REPORT, data, http, require, wait_for
from turn_helpers import submit, settled


def run(cluster):
    a = cluster.desktops['Desktop-A']
    old = a['node']
    before = str(uuid.uuid4())
    first = submit(cluster, 'Desktop-A', old, before, 'Reply exactly BEFORE_OWN_REGISTRATION_REMOVAL.')
    before_history = wait_for(lambda: settled(cluster, 'Desktop-A', before, first['turnId']), 'Own Desktop pre-removal task', seconds=180)
    wait_for(lambda: cluster.history('Desktop-B', before)['data'] == before_history, 'Own Desktop historical binding shared')

    cluster.desktop('Desktop-A', 'computer/delete', {'nodeId': old})
    # Read private state before any UI/identity/status request can repair it.
    owner = cluster.users['alice']['user']['user_id']
    state = json.loads((a['home'] / '.worker-state.json').read_text())
    require(state['removed_owner'] == owner, 'SDK deletion returned before persisting local removal')
    require(cluster.database(a['home'], 'SELECT finalized FROM local_computer_retirements WHERE node_id = ?', (old,)) == [(1,)],
            'SDK deletion deferred local alias retirement until a later poll')
    require(cluster.desktop('Desktop-A', 'desktop/workerIdentity')['nodeId'] == 'embedded', 'Deleted local UUID remains exposed')
    wait_for(lambda: cluster.desktop('Desktop-A', 'sync/status')['connection'] == 'connected', 'Removed Desktop keeps account Sync usable')
    def no_old_presence():
        metrics = data(http(cluster.url + '/v1/sync/metrics', token=cluster.users['alice']['access_token']))
        return all(old not in peer['computerIds'] for peer in metrics['peers'])
    wait_for(no_old_presence, 'Removed local UUID no longer advertised in Sync presence')

    parent = cluster.desktop('Desktop-A', 'fs/directory/read', {'nodeId': 'embedded'})['directory']['path']
    directory = cluster.desktop('Desktop-A', 'fs/directory/create', {'nodeId': 'embedded', 'parent': parent, 'name': 'post-removal-' + uuid.uuid4().hex})['path']
    project = cluster.desktop('Desktop-A', 'project/create', {'nodeId': 'embedded', 'name': 'New local work after removal', 'directory': directory})['project']
    after = str(uuid.uuid4())
    marker = 'LOCAL_AFTER_REMOVAL_' + uuid.uuid4().hex
    task = submit(cluster, 'Desktop-A', 'embedded', after,
        f'Use Bash to write exactly {marker} to local-after-removal.txt in the current workspace. Reply exactly {marker}.', projectId=project['project_id'])
    after_history = wait_for(lambda: settled(cluster, 'Desktop-A', after, task['turnId']), 'Unregistered local execution remains usable', seconds=180)
    proof_files = list(a['home'].rglob('local-after-removal.txt'))
    require(len(proof_files) == 1 and proof_files[0].read_text().strip() == marker, 'Unregistered local Worker did not preserve its Workspace write')
    require(cluster.desktop('Desktop-A', 'session/get', {'sessionId': after})['session']['nodeId'] == 'embedded', 'New local Session inherited deleted UUID')
    require(cluster.desktop('Desktop-A', 'project/read', {'project_id': project['project_id']})['project']['nodeId'] == 'embedded', 'New local Project inherited deleted UUID')
    require(not cluster.database(cluster.desktops['Desktop-B']['home'], 'SELECT session_key FROM sessions WHERE session_key = ?', (after,)), 'Unregistered local work leaked through the retired alias')
    print('CASE sync_own_deletion_sdk_persists_state_and_preserves_new_local_execution PASS', flush=True)

    fresh = cluster.desktop('Desktop-A', 'desktop/workerEnroll', {})['nodeId']
    require(fresh not in (old, 'embedded'), 'Explicit reconnect reused a removed UUID')
    a['node'] = fresh
    cluster.desktop('Desktop-A', 'sync/status')
    wait_for(lambda: cluster.history('Desktop-B', after)['data'] == after_history, 'New local work shared only after fresh enrollment')
    for host in ('Desktop-A', 'Desktop-B', 'Desktop-C'):
        wait_for(lambda host=host: bool(cluster.history(host, before)['data']), host + ' old history retained')
        require(cluster.desktop(host, 'session/get', {'sessionId': before})['session']['nodeId'] == old, host + ' rewrote historical Computer binding')
        wait_for(lambda host=host: cluster.history(host, after)['data'] == after_history, host + ' fresh history received')
        require(cluster.desktop(host, 'session/get', {'sessionId': after})['session']['nodeId'] == fresh, host + ' new work bound to removed UUID')
        require(cluster.desktop(host, 'project/read', {'project_id': project['project_id']})['project']['nodeId'] == fresh, host + ' new Project bound to removed UUID')
    print('CASE sync_own_deletion_new_work_uses_fresh_uuid_and_keeps_old_history_binding PASS', flush=True)

    # Now delete A from B and do not poll A. Its Sync subscription must persist
    # removal independently, including when the tombstone arrived before UI reads.
    cluster.desktop('Desktop-B', 'computer/delete', {'nodeId': fresh})
    wait_for(lambda: cluster.database(a['home'], 'SELECT finalized FROM local_computer_retirements WHERE node_id = ?', (fresh,)) == [(1,)],
             'Own imported deletion retired without source UI polling')
    require(json.loads((a['home'] / '.worker-state.json').read_text())['removed_owner'] == owner, 'Imported own deletion did not mark private state')
    wait_for(lambda: cluster.desktop('Desktop-A', 'sync/status')['connection'] == 'connected', 'Imported own deletion finished Sync reattachment')
    a['host'].close()
    a['host'] = Desktop(cluster, a['home'], 'Desktop-A')
    a['token'] = a['host'].call('auth/session/read', {})['session']['access_token']
    require(cluster.desktop('Desktop-A', 'desktop/workerIdentity')['nodeId'] == 'embedded', 'Restart resurrected imported removed UUID')
    for host in ('Desktop-A', 'Desktop-B', 'Desktop-C'):
        ids = {node['nodeId'] for node in cluster.desktop(host, 'computer/list', {'limit': 100})['data']}
        require(not {old, fresh}.intersection(ids), host + ' resurrected own removed registration')
        require(cluster.history(host, before)['data'] == before_history, host + ' lost original local history')
        require(cluster.history(host, after)['data'] == after_history, host + ' lost post-removal history')
    (REPORT / 'own-deletion-summary.json').write_text(json.dumps({'oldUuid': old, 'freshUuid': fresh,
        'sdkRemovalPersistedBeforePolling': True, 'newLocalExecutionRetained': True,
        'newWorkBoundOnlyToFreshEnrollment': True, 'importedOwnDeletionWithoutPolling': True,
        'restartDoesNotResurrect': True}, indent=2))
    print('CASE sync_own_imported_deletion_without_ui_polling_and_restart PASS', flush=True)
