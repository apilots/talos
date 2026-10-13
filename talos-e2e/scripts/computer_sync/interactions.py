"""Actual current-coordinator commands, source files and child resource callbacks."""
import base64
import json
import subprocess
import uuid
from pathlib import Path
from cluster import REPORT, ROOT, require, wait_for
from turn_helpers import submit, settled


def require_successful_wait(history, marker, child_session=None):
    tools = [block['content']['data'] for turn in history for message in turn['result'].get('result', {}).get('messages', [])
             for block in message['blocks'] if block['content']['type'] == 'toolResult' and block['content']['data']['toolName'] == 'wait_agent']
    require(bool(tools) and all(not tool['isError'] for tool in tools), 'wait_agent was missing or failed')
    snapshots = [part['result']['result'] for tool in tools for part in tool['content']
                 if part['type'] == 'subagent_result' and part['result']['operation'] == 'waitAgent']
    require(any(snapshot['outcome']['status'] == 'completed' and marker in snapshot['outcome']['output']
                and (child_session is None or snapshot['subagent']['sessionId'] == child_session) for snapshot in snapshots),
            'wait_agent did not return the actual child terminal result')


def shared(cluster, session):
    for host in ('Desktop-A', 'Desktop-B', 'G'):
        wait_for(lambda host=host: bool(cluster.history(host, session)['data']), host + ' shared Session')


def source_files(cluster):
    body = (ROOT / 'build/desktop/icons/icon.png').read_bytes()
    uploaded = cluster.desktop('Desktop-A', 'file/upload', {
        'request': {'filename': 'source-only.png', 'scope': {'type': 'personal'}, 'tags': [], 'metadata': {}},
        'dataBase64': base64.b64encode(body).decode()})['file']
    request = {'fileId': uploaded['fileId']}
    wait_for(lambda: any(f['fileId'] == request['fileId'] for f in cluster.desktop('Desktop-B', 'file/list')['data']), 'Source file metadata synchronized')
    fetched = cluster.desktop('Desktop-B', 'file/content', request)
    require(base64.b64decode(fetched['dataBase64']) == body, 'Remote FileService changed source bytes')
    cluster.desktop('Desktop-B', 'file/update', {**request, 'tags': ['remote-source-update'], 'metadata': {'source': 'B'}})
    wait_for(lambda: cluster.desktop('Desktop-A', 'file/read', request)['file']['tags'] == ['remote-source-update'], 'File update reached source')
    source = cluster.desktops['Desktop-A']
    source['host'].close()
    reply = cluster.desktops['Desktop-B']['host'].request('file/content', request, cluster.desktops['Desktop-B']['token'])
    require(reply.get('error', {}).get('code') == 'serviceUnavailable', 'Offline source bytes must fail explicitly through FileService')
    from cluster import Desktop
    source['host'] = Desktop(cluster, source['home'], 'Desktop-A')
    source['token'] = source['host'].call('auth/session/read')['session']['access_token']
    wait_for(lambda: any(n['nodeId'] == source['node'] and n['available'] for n in cluster.list_nodes('alice')), 'Source Worker Connect restored')
    wait_for(lambda: cluster.desktop('Desktop-A', 'sync/status')['connection'] == 'connected', 'Source management connection restored')
    require(base64.b64decode(cluster.desktop('Desktop-B', 'file/content', request)['dataBase64']) == body, 'Source restart lost attachment')
    cluster.desktop('Desktop-B', 'file/delete', request)
    wait_for(lambda: not any(f['fileId'] == request['fileId'] for f in cluster.desktop('Desktop-A', 'file/list')['data']), 'Source deletion synchronized')
    print('CASE sync_source_files_read_update_offline_restart_delete PASS', flush=True)


def commands(cluster):
    node = cluster.desktops['Desktop-A']['node']
    home = cluster.desktops['Desktop-A']['home']
    session = str(uuid.uuid4())
    request_id = str(uuid.uuid4())
    first = submit(cluster, 'Desktop-A', node, session, 'Reply exactly SHARED_COMMAND_BASE.', clientRequestId=request_id)
    wait_for(lambda: settled(cluster, 'Desktop-A', session, first['turnId']), 'Initial command settled', 120)
    shared(cluster, session)
    replay = submit(cluster, 'Desktop-B', node, session, 'Reply exactly SHARED_COMMAND_BASE.', clientRequestId=request_id)
    require(replay == first, 'Other coordinator did not replay the exact committed command response')
    started = 'command-started-' + uuid.uuid4().hex
    forbidden = 'command-forbidden-' + uuid.uuid4().hex
    ack = submit(cluster, 'Desktop-B', node, session,
                 f'Use Bash once to run: touch {started}; sleep 30; touch {forbidden}. Do not run another tool.')
    wait_for(lambda: list(home.rglob(started)), 'Remote current-coordinator real tool started', 90)
    active = wait_for(lambda: cluster.desktop('Desktop-A', 'session/active_turn', {'sessionId': session})['activeTurn'], 'Other host received active state')
    require(active['identity']['turnId'] == ack['turnId'], 'Wrong current Turn')
    denied = cluster.desktops['Desktop-A']['host'].request('turn/start', {
        'nodeId': node, 'sessionId': session, 'input': {'content': [{'type': 'text', 'text': 'Must not execute concurrently.'}]},
        'modelSelection': {'modelRef': {'provider': cluster.model['provider'], 'id': cluster.model['id']}, 'reasoningEffort': None},
        'userPermissionMode': 'fullAccess'}, cluster.desktops['Desktop-A']['token'])
    require(denied.get('error', {}).get('code') in ('conflict', 'activeTurnRunning'),
            'A second control plane entered the running Session: ' + str(denied.get('error', {}).get('code')))
    queued = cluster.desktop('Desktop-A', 'messageQueue/add', {'sessionId': session, 'input': {'content': [{'type': 'text', 'text': 'queued through source host'}]}})
    require(cluster.desktop('Desktop-B', 'messageQueue/list', {'sessionId': session}) == queued, 'Queue did not reach current coordinator')
    cluster.desktop('Desktop-A', 'messageQueue/delete', {'sessionId': session, 'messageId': queued['messages'][0]['id']})
    identity = {'sessionId': session, 'turnId': ack['turnId']}
    cluster.desktop('Desktop-A', 'turn/input', {'identity': identity,
        'input': {'type': 'message', 'input': {'content': [{'type': 'text', 'text': 'Live input through another control plane.'}]}},
        'clientRequestId': str(uuid.uuid4())})
    interrupted = cluster.desktop('Desktop-A', 'turn/interrupt', {'identity': identity})
    require(interrupted['identity'] == identity and interrupted['outcome']['type'] == 'cancelled', 'Exact Turn cancellation did not return to current coordinator')
    wait_for(lambda: cluster.desktop('Desktop-B', 'session/active_turn', {'sessionId': session})['activeTurn'] is None, 'Current coordinator cleanup settled')
    require(not list(home.rglob(forbidden)), 'Cancelled remote tool produced its delayed side effect')
    wait_for(lambda: cluster.history('Desktop-A', session)['data'] == cluster.history('Desktop-B', session)['data'], 'Cancellation facts synchronized')
    print('CASE sync_current_coordinator_input_queue_interrupt_exclusion_and_idempotency PASS', flush=True)


def question_and_child(cluster):
    node = cluster.nodes['Docker-Worker']['node_id']
    home = cluster.nodes['Docker-Worker']['home']
    session = str(uuid.uuid4())
    first = submit(cluster, 'Desktop-A', node, session, 'Reply exactly QUESTION_BASE.')
    wait_for(lambda: settled(cluster, 'Desktop-A', session, first['turnId']), 'Question base settled', 120)
    shared(cluster, session)
    question = submit(cluster, 'Desktop-B', node, session,
        'Call AskUserQuestion exactly once. Header Execution Mode, question Choose mode; '
        'options: id plan, label Plan, description Explore; id direct, label Direct, description Execute. '
        'After the answer reply exactly CROSS_COORDINATOR_QUESTION_OK.')
    wait_for(lambda: (cluster.desktop('Desktop-A', 'session/active_turn', {'sessionId': session})['activeTurn'] or {}).get('state') == 'waitingForInput',
        'Remote waiting state synchronized', 90)
    events = cluster.events('alice', session, question['turnId'], 90)
    require(events[-1]['payload']['type'] == 'waitingForInput', 'Actual remote question did not wait')
    action = events[-1]['payload']['data']['action']
    require(action['params']['type'] == 'question', 'Expected typed question')
    # The waiting continuation protects configuration on every management host.
    for host in ('Desktop-A', 'Desktop-B'):
        for method in ('config/update', 'config/delete', 'config/reload'):
            original = cluster.desktop(host, 'config/get', {'nodeId': node, 'path': 'llm'})['config']
            params = {'nodeId': node}
            if method == 'config/update': params['value'] = {'type': 'llm', 'value': original}
            if method == 'config/delete': params['path'] = 'llm'
            denied = cluster.desktops[host]['host'].request(method, params, cluster.desktops[host]['token'])
            require(denied.get('error', {}).get('code') in ('conflict', 'activeTurnRunning'), 'Waiting Turn allowed configuration mutation via ' + host + ' ' + method)
    response = cluster.desktop('Desktop-A', 'turn/input', {'identity': {'sessionId': session, 'turnId': question['turnId']},
        'input': {'type': 'userActionResponse', 'actionId': action['actionId'],
                  'response': {'type': 'question', 'response': {'type': 'selectedOption', 'optionId': 'plan'}}},
        'clientRequestId': str(uuid.uuid4())})
    require(response['identity']['turnId'] == question['turnId'], 'Question resumed a different Turn')
    history = wait_for(lambda: settled(cluster, 'Desktop-B', session, question['turnId']), 'Question response returned to coordinator', 120)
    require('CROSS_COORDINATOR_QUESTION_OK' in json.dumps(history[-1]['result']), 'Answer was not received')
    marker = 'CHILD_' + uuid.uuid4().hex
    filename = 'child-' + uuid.uuid4().hex + '.txt'
    child = submit(cluster, 'Desktop-B', node, session,
        'Use spawn_agent exactly once to create child named writer. Delegate this exact task: '
        f'Use Bash to sleep 12 seconds then write {marker} to {filename}, reply {marker}. '
        'Immediately wait_agent target writer timeoutMs 60000. After successful child completion '
        f'use Bash to read {filename} and reply with its contents. Do not perform the child task yourself.')
    try:
        history = wait_for(lambda: settled(cluster, 'Desktop-B', session, child['turnId']), 'Remote child resource task settled', 150)
    except AssertionError:
        child_ids = cluster.database(cluster.desktops['Desktop-B']['home'],
            'SELECT session_key FROM sessions WHERE parent_session_id=?', (session,))
        diagnostic = {'parentHistory': cluster.history('Desktop-B', session), 'children': []}
        for (child_id,) in child_ids:
            diagnostic['children'].append({'id': child_id, 'events': [json.loads(row[0]) for row in cluster.database(cluster.desktops['Desktop-B']['home'],
                    'SELECT payload_json FROM agent_events WHERE session_id=? ORDER BY event_seq', (child_id,))],
                'claim': cluster.database(cluster.desktops['Desktop-B']['home'],
                    'SELECT turn_id,state,last_event_seq FROM agent_active_turns WHERE session_id=?', (child_id,))})
        (REPORT / 'child-timeout-diagnostic.json').write_text(json.dumps(diagnostic, indent=2))
        raise
    require(marker in json.dumps(history[-1]['result']), 'Parent lost child result')
    children = cluster.database(cluster.desktops['Desktop-B']['home'], 'SELECT session_key, execution_node_id FROM sessions WHERE parent_session_id=?', (session,))
    require(len(children) == 1 and children[0][1] == node, 'Child was not persisted by current coordinator on the same Computer')
    for host in ('Desktop-A', 'G'):
        wait_for(lambda host=host: cluster.history(host, session)['data'] == history, 'Full child/parent history synchronized')
    require(len(list(home.rglob(filename))) == 1, 'Child file did not land on Docker Worker')
    require_successful_wait(history, marker, children[0][0])
    (REPORT / 'resource-history.json').write_text(json.dumps(history, indent=2))
    print('CASE sync_remote_question_child_result_and_computer_affinity PASS', flush=True)

    # Reset advances the original history generation; imported old facts must not
    # reappear. The same immutable Computer can then run a new Turn from A.
    cluster.desktop('Desktop-B', 'session/clear', {'sessionId': session})
    for host in ('Desktop-A', 'G'):
        wait_for(lambda host=host: cluster.history(host, session)['data'] == [], 'Cleared generation propagated')
    reset = submit(cluster, 'Desktop-A', node, session, 'Reply exactly RESET_GENERATION_OK.')
    fresh = wait_for(lambda: settled(cluster, 'Desktop-A', session, reset['turnId']), 'Same Computer continued after reset', 120)
    require(len(fresh) == 1, 'Old generation reappeared after clear')
    wait_for(lambda: cluster.history('Desktop-B', session)['data'] == fresh, 'New generation synchronized')
    cluster.desktop('Desktop-B', 'session/delete', {'sessionId': session})
    identities = [session] + [row[0] for row in children]
    for host in ('Desktop-A', 'Desktop-B'):
        wait_for(lambda host=host: not cluster.database(cluster.desktops[host]['home'],
            'SELECT session_key FROM sessions WHERE session_key IN (' + ','.join('?' for _ in identities) + ')', identities), 'Parent and child tombstones propagated')
    wait_for(lambda: not cluster.query('SELECT session_key FROM sessions WHERE session_key=?', (session,)), 'G removed parent')
    subprocess.run(['docker', 'stop', '--time', '10', cluster.hub_container], check=True, capture_output=True)
    subprocess.run(['docker', 'start', cluster.hub_container], check=True, capture_output=True)
    for host in ('Desktop-A', 'Desktop-B'):
        wait_for(lambda host=host: cluster.desktop(host, 'sync/status')['connection'] == 'connected', 'Hub restart checkpoint restored')
        require(not cluster.database(cluster.desktops[host]['home'],
            'SELECT session_key FROM sessions WHERE session_key IN (' + ','.join('?' for _ in identities) + ')', identities), 'Deleted tree reappeared after hub restart')
    print('CASE sync_clear_same_computer_continuation_and_deleted_tree_stay_deleted_after_hub_restart PASS', flush=True)


def waiting_plugin_mutations_with_sync_offline(cluster):
    node = cluster.desktops['Desktop-A']['node']
    session = str(uuid.uuid4())
    base = submit(cluster, 'Desktop-B', node, session, 'Reply exactly PLUGIN_GUARD_BASE.')
    wait_for(lambda: settled(cluster, 'Desktop-B', session, base['turnId']), 'Plugin guard base completed', 120)
    wait_for(lambda: cluster.history('Desktop-A', session)['data'] == cluster.history('Desktop-B', session)['data'], 'Plugin guard base shared')
    subprocess.run(['docker', 'stop', '--time', '10', cluster.hub_container], check=True, capture_output=True)
    try:
        question = submit(cluster, 'Desktop-B', node, session,
            'Call AskUserQuestion exactly once. Header Plugin Settings, question Choose mode; '
            'options: id plan, label Plan, description Explore; id direct, label Direct, description Execute. '
            'After the answer reply exactly PLUGIN_WAIT_OK.')
        wait_for(lambda: (cluster.desktop('Desktop-B', 'session/active_turn', {'sessionId': session})['activeTurn'] or {}).get('state') == 'waitingForInput', 'Offline sync Worker waiting', 90)
        replay = cluster.desktop('Desktop-B', 'timeline/recover', {'sessionId': session})['replay']['events']
        action = next(event['event']['data']['payload']['data']['action'] for event in reversed(replay)
                      if event['event']['type'] == 'agent' and event['event']['data']['payload']['type'] == 'waitingForInput')
        require(not cluster.database(cluster.desktops['Desktop-A']['home'], 'SELECT 1 FROM agent_active_turns WHERE session_id = ?', (session,)),
                'Origin management DB unexpectedly knows the offline waiting Turn')
        params = {'json': json.dumps({'command': '/bin/echo', 'args': ['guard']}),
                  'name': 'waiting-guard-' + uuid.uuid4().hex, 'enable': False, 'conflictPolicy': 'reject'}
        for target in ({}, {'nodeId': node}):
            denied = cluster.desktops['Desktop-A']['host'].request('mcp/server/import', {**params, **target}, cluster.desktops['Desktop-A']['token'])
            require(denied.get('error', {}).get('code') in ('conflict', 'activeTurnRunning'), 'Waiting Worker allowed MCP mutation through stale origin DB')
        identity = {'sessionId': session, 'turnId': question['turnId']}
        interrupted = cluster.desktop('Desktop-B', 'turn/interrupt', {'identity': identity})
        require(interrupted['identity'] == identity and interrupted['outcome']['type'] == 'cancelled',
                'Waiting cancellation did not reach the original Worker and coordinator')
        wait_for(lambda: cluster.desktop('Desktop-B', 'session/active_turn', {'sessionId': session})['activeTurn'] is None,
                 'Waiting cancellation terminal cleanup', 90)
        terminal = cluster.history('Desktop-B', session)['data'][-1]
        require(terminal['identity'] == identity and terminal['result']['status'] == 'failed'
                and terminal['result']['result']['failure']['error']['code'] == 'cancelled',
                'Waiting cancellation did not persist its exact terminal result')
        installed = cluster.desktop('Desktop-A', 'mcp/server/import', {**params, 'nodeId': node})
        require(Path(installed['install']['installed_path']).is_relative_to(cluster.desktops['Desktop-A']['home']), 'MCP was installed outside the target Worker')
        cluster.desktop('Desktop-A', 'plugin/uninstall', {'nodeId': node, 'install_id': installed['install']['id']})
    finally:
        subprocess.run(['docker', 'start', cluster.hub_container], check=True, capture_output=True)
    wait_for(lambda: cluster.history('Desktop-A', session)['data'] == cluster.history('Desktop-B', session)['data'], 'Offline waiting facts recovered')
    print('CASE sync_offline_waiting_plugin_mutation_alias_uuid_and_terminal_release PASS', flush=True)
