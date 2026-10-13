"""Actual Worker filesystem, Project and pinned Workspace acceptance through product APIs."""
import uuid
import os
import signal
from urllib.parse import urlencode
from pathlib import Path
import yaml
from fixture import Cluster, data, http, require, wait_for
from scenarios import passed


class WorkspaceCluster(Cluster):
    def config(self, home, node_config):
        env = super().config(home, node_config)
        if 'runner' in node_config:
            path = Path(env['TALOS_CONFIG'])
            config = yaml.safe_load(path.read_text())
            config['workspaces'] = {
                'default_workspace_id': f'pc-{home.name}',
                'definitions': [
                    {'workspace_id': f'pc-{home.name}', 'root_prefix': 'pc-files'},
                    {'workspace_id': 'next', 'root_prefix': 'next-files'},
                ],
            }
            path.write_text(yaml.safe_dump(config, sort_keys=False))
            # Each real test host has its own configured HOME for the directory picker.
            env['HOME'] = str(home)
        return env


def workspace(cluster):
    token = cluster.users['alice']['access_token']
    selected = cluster.nodes['alice-b']
    node_id = selected['node_id']
    query = urlencode({'nodeId': node_id})
    info = data(http(f'{cluster.url}/v1/workspace/info?{query}', token=token))
    require(info['workspaceId'] == f'pc-{selected["home"].name}', 'Control-plane Workspace default leaked into Worker')
    require(Path(info['userWorkspaceDir']).is_relative_to(selected['home']), 'Workspace root is not on selected Worker')
    directory = data(http(f'{cluster.url}/v1/fs/directory?{query}', token=token))['directory']
    require(Path(directory['path']) == selected['home'], 'Directory picker ran on control plane')
    require(data(http(f'{cluster.url}/v1/models?{query}', token=token))['models'], 'Selected Worker model catalog missing')
    bob_query = urlencode({'nodeId': cluster.nodes['bob-a']['node_id']})
    bob_directory = data(http(f'{cluster.url}/v1/fs/directory?{bob_query}', token=cluster.users['bob']['access_token']))['directory']
    require(Path(bob_directory['path']) == cluster.nodes['bob-a']['home'], 'Bob directory did not use his Computer')
    passed('selected_worker_workspace_directory_models_and_owner')

    name = 'project-' + uuid.uuid4().hex
    created = data(http(f'{cluster.url}/v1/fs/directory', {'nodeId': node_id, 'parent': str(selected['home']), 'name': name}, token))
    project = data(http(f'{cluster.url}/v1/projects', {'nodeId': node_id, 'name': 'Remote project', 'directory': created['path']}, token))['project']
    require(project['nodeId'] == node_id and project['status']['state'] == 'available', 'Project affinity/status missing')
    require(not (cluster.server_home / name).exists(), 'Directory was created on control plane')
    listed = data(http(f'{cluster.url}/v1/projects?{query}', token=token))['projects']
    require([item['project_id'] for item in listed] == [project['project_id']], 'Project list ignored Computer filter')
    passed('worker_directory_creates_node_owned_project')

    marker = uuid.uuid4().hex
    filename = 'result-' + marker + '.txt'
    session, answer, events = cluster.turn('alice', f"Use Bash once to run: printf '{marker}' > {filename}. Then reply exactly {marker}.", node_id=node_id, project_id=project['project_id'])
    require(marker in answer and (Path(created['path']) / filename).read_text() == marker, 'Turn did not execute in selected Project')
    require(any(event['payload']['type'] == 'messageBlockDelta' for event in events), 'No live feedback from Worker')
    stored = cluster.query('SELECT execution_node_id, workspace_id, project_id FROM sessions WHERE session_key=?', (session,))
    require(stored == [(node_id, info['workspaceId'], project['project_id'])], 'Session did not persist target Worker defaults')
    sessions = data(http(f'{cluster.url}/v1/sessions?{query}', token=token))['sessions']
    require(any(item['id'] == session and item['nodeId'] == node_id for item in sessions), 'Session node projection/filter missing')
    other_query = urlencode({'nodeId': cluster.nodes['alice-a']['node_id']})
    require(all(item['id'] != session for item in data(http(f'{cluster.url}/v1/sessions?{other_query}', token=token))['sessions']), 'Session appeared on another Computer')
    _, rejected = cluster.start_turn('alice', 'Never execute this.', session, node_id=cluster.nodes['alice-a']['node_id'])
    require(rejected.get('error', {}).get('code') == 'invalidRequest', 'Existing Session silently changed Computer')
    _, rejected = cluster.start_turn('alice', 'Never execute this.', node_id=cluster.nodes['alice-a']['node_id'], project_id=project['project_id'])
    require(rejected.get('error', {}).get('code') == 'invalidRequest', 'Project and Computer mismatch was accepted')
    passed('project_turn_worker_file_timeline_and_immutable_session')

    # Keep the registered channel open while the actual execution process stops responding.
    foreign_before = http(f'{cluster.url}/v1/projects/{project["project_id"]}', token=cluster.users['bob']['access_token'])
    require(foreign_before.get('status') == 'error', 'Foreign owner acquired the Project')
    os.kill(selected['process'].pid, signal.SIGSTOP)
    try:
        timed_out = data(http(f'{cluster.url}/v1/projects?{query}', token=token))['projects']
        require(len(timed_out) == 1 and timed_out[0]['project_id'] == project['project_id'] and timed_out[0]['status']['state'] == 'nodeUnavailable', 'RPC timeout hid persisted Project metadata')
        denied = http(f'{cluster.url}/v1/projects/{project["project_id"]}', token=cluster.users['bob']['access_token'])
        require(denied.get('error', {}).get('code') == foreign_before['error']['code'], 'Availability handling changed the existing foreign-owner error')
    finally:
        os.kill(selected['process'].pid, signal.SIGCONT)
    passed('project_rpc_timeout_retains_metadata_without_swallowing_owner_error')

    cluster.stop(selected['process'])
    offline = data(http(f'{cluster.url}/v1/projects/{project["project_id"]}', token=token))['project']
    require(offline['status']['state'] == 'nodeUnavailable', 'Offline Project metadata became inaccessible or used local filesystem')
    config_path = Path(selected['env']['TALOS_CONFIG'])
    config = yaml.safe_load(config_path.read_text())
    config['workspaces']['default_workspace_id'] = 'next'
    config_path.write_text(yaml.safe_dump(config, sort_keys=False))
    cluster.start_worker('alice-b')
    wait_for(lambda: cluster.info('alice-b').get('available'), 'Worker restart with changed default')
    cluster.turn('alice', f'Read {filename} using Bash and reply with its content.', session)
    require(cluster.query('SELECT workspace_id FROM sessions WHERE session_key=?', (session,)) == [(info['workspaceId'],)], 'Changed default rebound an existing Session')
    fresh, _, _ = cluster.turn('alice', 'Reply exactly NEW_DEFAULT.', node_id=node_id)
    require(cluster.query('SELECT workspace_id FROM sessions WHERE session_key=?', (fresh,)) == [('next',)], 'New Session did not use changed Worker default')
    passed('offline_metadata_restart_and_saved_workspace_binding')

    cluster.stop(selected['process'])
    config['workspaces']['definitions'] = [item for item in config['workspaces']['definitions'] if item['workspace_id'] == 'next']
    config_path.write_text(yaml.safe_dump(config, sort_keys=False))
    cluster.start_worker('alice-b')
    wait_for(lambda: cluster.info('alice-b').get('available'), 'Worker restart after Workspace removal')
    _, rejected = cluster.start_turn('alice', 'Never execute this.', session)
    require(rejected.get('error', {}).get('code') == 'invalidRequest', 'Removed saved Workspace fell back to a new default')
    require(data(http(f'{cluster.url}/v1/sessions/{session}', token=token))['session']['nodeId'] == node_id, 'Session metadata disappeared after Workspace removal')
    passed('removed_workspace_fails_without_rebinding')
