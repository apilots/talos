"""Actual current-coordinator commands, source files and child resource callbacks."""
import base64
import json
import hashlib
import queue
import subprocess
import time
import uuid
from pathlib import Path
import yaml
from google.protobuf import json_format, message_factory
from cluster import REPORT, ROOT, Desktop, free_port, require, wait_for
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


def failed_direct_registration(cluster):
    mac_config = yaml.safe_load((cluster.nodes['Mac-Worker']['home'] / 'conf/talos.yaml').read_text())
    mac_port = mac_config['agent_node']['worker_rpc']['bind_address'].rsplit(':', 1)[1]
    deployments = cluster.manage('alice', 'GetDeployments', {'node_id': cluster.nodes['Mac-Worker']['node_id']})['deployments']['deployments']
    # The real Mac listener's certificate covers 127.0.0.1, not localhost.
    for endpoint in (f'https://127.0.0.1:{free_port()}', f'https://localhost:{mac_port}'):
        created = cluster.manage('alice', 'CreateNode', {
            'display_name': 'Rejected Direct', 'endpoint': endpoint,
            'credential_expires_at': int(time.time()) + 3600})['created']
        node = created['node']['node_id']
        result = cluster.rpc('AgentNodeControl', 'Register', {
            'auth': {'node_id': node, 'token': created['credential']['token']},
            'params': {'deployments': deployments}
        })
        require(result.get('failure', {}).get('error', {}).get('code') == 'ERROR_CODE_SERVICE_UNAVAILABLE',
                'Invalid Direct endpoint was published instead of rejecting registration')
        require(not cluster.manage('alice', 'GetNode', {'node_id': node})['node'].get('available', False),
                'Failed Direct registration advertised Connected')
        cluster.manage('alice', 'SetComputerStatus', {'node_id': node, 'status': 'NODE_STATUS_DISABLED'})
    print('CASE sync_direct_registration_requires_reverse_tls_reachability PASS', flush=True)


def abandoned_cross_coordinator_admission(cluster, node, session):
    source = cluster.desktops['Desktop-B']
    peer = cluster.database(source['home'], 'SELECT peer_id FROM sync_identity WHERE singleton = 1')[0][0]
    agent, workspace, user = cluster.database(cluster.desktops['Desktop-A']['home'],
        'SELECT agent_id, workspace_id, COALESCE(workspace_user_id, user_id) FROM sessions WHERE session_key = ?', (session,))[0]
    context = {'agent_id': agent, 'session_id': session, 'workspace_id': workspace, 'user_id': user}
    marker = cluster.desktops['Desktop-A']['home'] / 'data/worker/session-heads' / (hashlib.sha256(session.encode()).hexdigest() + '.json')
    wait_for(lambda: json.loads(marker.read_text())['visibility'] == 'Shared', 'Original Worker history is Shared')
    before = json.loads(marker.read_text())
    method = cluster.pool.FindMethodByName('talos.agent.v1.AgentCoordinatorRpc.Connect')
    request_type = message_factory.GetMessageClass(method.input_type)
    response_type = message_factory.GetMessageClass(method.output_type)
    sent = queue.Queue()
    sent.put(json_format.ParseDict({'connect': {'peer_id': peer, 'computer_id': node}}, request_type()))
    connect = cluster.channel.stream_stream('/talos.agent.v1.AgentCoordinatorRpc/Connect',
        request_serializer=request_type.SerializeToString, response_deserializer=response_type.FromString)
    stream = connect(iter(sent.get, None), timeout=30,
                     metadata=(('authorization', 'Bearer ' + source['token']),))
    try:
        require(next(stream).WhichOneof('body') == 'connected', 'B could not open its authenticated Coordinator Connect')
        # Prevent background sharing from masking a failed reservation rollback.
        subprocess.run(['docker', 'stop', '--time', '10', cluster.hub_container], check=True, capture_output=True)
        source['host'].close()
        turn = str(uuid.uuid4())
        forbidden = 'abandoned-admission-' + uuid.uuid4().hex
        now = int(time.time() * 1000)
        model = cluster.model
        params = {'context': {'session': context, 'turn_id': turn, 'history_cursor': before['head'],
                  'model': {'provider': model['provider'], 'id': model['id'], 'name': model['name'],
                            'context_window': model['context_window'], 'reasoning': model['reasoning'],
                            'reasoning_effort': 'REASONING_EFFORT_' + model['reasoning_effort'].upper(),
                            'speed': 'MODEL_SPEED_' + model['speed'].upper(),
                            'input': ['MODEL_INPUT_KIND_' + kind.upper() for kind in model['input']],
                            'cost': model['cost']},
                  'model_selection': {'provider': model['provider'], 'model_id': model['id']},
                  'permission_mode': 'PERMISSION_MODE_FULL_ACCESS'},
                  'input': {'message_id': str(uuid.uuid4()), 'turn_id': turn, 'role': 'MESSAGE_ROLE_USER',
                    'status': 'MESSAGE_STATUS_COMPLETED', 'created_at': now, 'completed_at': now,
                    'blocks': [{'block_id': str(uuid.uuid4()), 'status': 'BLOCK_STATUS_COMPLETED',
                                'started_at': now, 'completed_at': now,
                                'user_content': {'text': 'Use Bash to touch ' + forbidden}}]}}
        sent.put(json_format.ParseDict({'call': {'call_id': 1, 'sequence': 1, 'timeout_ms': 15000,
            'response_window': 8, 'start': {'auth': {'node_id': node, 'token': source['token']},
                                          'params': {'run': params}}}}, request_type()))
        result = json_format.MessageToDict(next(stream), preserving_proto_field_name=True).get('result', {})
        require('admission' in result.get('run', {}), 'B did not receive a real paused Harness admission')
        stream.cancel()
        sent.put(None)
        wait_for(lambda: json.loads(marker.read_text()) == before, 'Unconsumed admission restored the complete original Worker marker')
        require(not list(cluster.desktops['Desktop-A']['home'].rglob(forbidden)), 'Unconsumed admission ran its tool')
        retry = submit(cluster, 'Desktop-A', node, session, 'Reply exactly ABANDONED_HANDOFF_RECOVERED.')
        history = wait_for(lambda: settled(cluster, 'Desktop-A', session, retry['turnId']), 'Original coordinator continued with B and H offline', 120)
        require(len(history) == 2 and 'ABANDONED_HANDOFF_RECOVERED' in json.dumps(history[-1]['result']),
                'Abandoned admission produced history or blocked the original coordinator')
    finally:
        stream.cancel()
        sent.put(None)
        subprocess.run(['docker', 'start', cluster.hub_container], check=True, capture_output=True)
        source['host'] = Desktop(cluster, source['home'], 'Desktop-B')
        source['token'] = source['host'].call('auth/session/read')['session']['access_token']
    wait_for(lambda: cluster.desktop('Desktop-B', 'sync/status')['connection'] == 'connected', 'B synchronization resumed')
    wait_for(lambda: cluster.history('Desktop-A', session)['data'] == cluster.history('Desktop-B', session)['data'], 'Only executed history synchronized after recovery')
    print('CASE sync_abandoned_cross_coordinator_admission_preserves_original_shared_history PASS', flush=True)


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
    abandoned_cross_coordinator_admission(cluster, node, session)
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
