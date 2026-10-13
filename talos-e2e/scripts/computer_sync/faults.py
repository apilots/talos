"""Real independent cloud outages and durable same-Worker history cursor recovery."""
import subprocess
import uuid
from cluster import Cluster, Desktop, require, wait_for
from turn_helpers import submit, settled
from interactions import shared


def restart_desktop(cluster, name):
    desktop = cluster.desktops[name]
    desktop['host'].close()
    desktop['host'] = Desktop(cluster, desktop['home'], name)
    desktop['token'] = desktop['host'].call('auth/session/read')['session']['access_token']
    wait_for(lambda: any(n['nodeId'] == desktop['node'] and n['available'] for n in cluster.list_nodes('alice')), 'Restarted Worker Connect restored')
    wait_for(lambda: cluster.desktop(name, 'sync/status')['localComputerId'] == desktop['node'], 'Saved Computer resumed')


def start_rejected(cluster, session, node):
    response = cluster.desktops['Desktop-A']['host'].request('turn/start', {
        'nodeId': node, 'sessionId': session,
        'input': {'content': [{'type': 'text', 'text': 'Old facts must not become a new Turn.'}]},
        'modelSelection': {'modelRef': {'provider': cluster.model['provider'], 'id': cluster.model['id']}, 'reasoningEffort': None},
        'userPermissionMode': 'fullAccess'}, cluster.desktops['Desktop-A']['token'])
    require(response.get('error', {}).get('code') == 'conflict', 'Old control-plane history overrode the Worker Local history cursor')


def clouds(cluster):
    node = cluster.desktops['Desktop-A']['node']
    session = str(uuid.uuid4())
    ack = submit(cluster, 'Desktop-A', node, session, 'Reply exactly BEFORE_CLOUD_OUTAGE.')
    wait_for(lambda: settled(cluster, 'Desktop-A', session, ack['turnId']), 'Base Turn committed', 120)
    shared(cluster, session)
    # G and H fail independently. H alone must not carry any execution request.
    cluster.stop(cluster.server)
    ack = submit(cluster, 'Desktop-A', node, session, 'Reply exactly LOCAL_DURING_G_OUTAGE.')
    wait_for(lambda: settled(cluster, 'Desktop-A', session, ack['turnId']), 'Local loopback during G outage', 120)
    wait_for(lambda: settled(cluster, 'Desktop-B', session, ack['turnId']), 'H synchronized facts without G', 90)
    subprocess.run(['docker', 'stop', '--time', '10', cluster.hub_container], check=True, capture_output=True)
    ack = submit(cluster, 'Desktop-A', node, session, 'Reply exactly LOCAL_WITH_BOTH_CLOUDS_DOWN.')
    wait_for(lambda: settled(cluster, 'Desktop-A', session, ack['turnId']), 'Local loopback with G and H stopped', 120)
    require(cluster.desktop('Desktop-A', 'sync/status')['pendingChanges'] > 0, 'Both-cloud outage lost local outbox')
    # Restart the existing G only; keep the H container stopped and DB unchanged.
    Cluster.start_server(cluster)
    wait_for(lambda: all(n.get('available') for n in cluster.list_nodes('alice')), 'Worker Connect leases recovered', 90)
    subprocess.run(['docker', 'start', cluster.hub_container], check=True, capture_output=True)
    wait_for(lambda: settled(cluster, 'Desktop-B', session, ack['turnId']), 'Both-cloud recovery caught up', 90)
    print('CASE sync_gateway_and_hub_outages_are_independent_of_local_execution PASS', flush=True)


def local_history_cursor(cluster):
    node = cluster.desktops['Desktop-A']['node']
    session = str(uuid.uuid4())
    ack = submit(cluster, 'Desktop-A', node, session, 'Reply exactly SHARED_HEAD_BASE.')
    wait_for(lambda: settled(cluster, 'Desktop-A', session, ack['turnId']), 'Shared history cursor base', 120)
    shared(cluster, session)
    subprocess.run(['docker', 'stop', '--time', '10', cluster.hub_container], check=True, capture_output=True)
    remote = submit(cluster, 'Desktop-B', node, session, 'Reply exactly LAST_LOCAL_HEAD_ON_B.')
    wait_for(lambda: settled(cluster, 'Desktop-B', session, remote['turnId']), 'B committed without H', 120)
    start_rejected(cluster, session, node)
    restart_desktop(cluster, 'Desktop-A')
    start_rejected(cluster, session, node)
    ack = submit(cluster, 'Desktop-B', node, session, 'Reply exactly LOCAL_HEAD_SURVIVED_WORKER_RESTART.')
    wait_for(lambda: settled(cluster, 'Desktop-B', session, ack['turnId']), 'Original coordinator recovered persisted Worker history cursor', 120)
    subprocess.run(['docker', 'start', cluster.hub_container], check=True, capture_output=True)
    wait_for(lambda: settled(cluster, 'Desktop-A', session, ack['turnId']), 'Complete history cursor facts caught up', 90)
    ack = submit(cluster, 'Desktop-A', node, session, 'Reply exactly SHARED_HANDOFF_RECOVERED.')
    history = wait_for(lambda: settled(cluster, 'Desktop-A', session, ack['turnId']), 'Caught-up source can coordinate again', 120)
    require(len(history) == 4, 'Rejected stale requests produced extra execution facts')
    print('CASE sync_last_local_history_cursor_survives_worker_restart_and_blocks_stale_control_plane PASS', flush=True)
