"""Observable multi-node acceptance scenarios, without database mutations."""

from concurrent.futures import ThreadPoolExecutor
import json
import os
import subprocess
import signal
import sys
import time
import traceback
import uuid
from datetime import datetime
from zoneinfo import ZoneInfo

import grpc

from fixture import Cluster, REPORT, ROOT, data, http, require, wait_for


def passed(name):
    print(f'CASE {name} PASS', flush=True)


def denied(response, code):
    actual = response.get('failure', {}).get('error', {}).get('code')
    require(actual == 'ERROR_CODE_' + code, f'Expected {code}, received {response}')


def registration(cluster):
    for user, expected in (('alice', {'alice-a', 'alice-b'}), ('bob', {'bob-a'})):
        page = cluster.manage(user, 'ListNodes', {'limit': 100})['page']
        nodes = {node['node_id'] for node in page['data'] if node['mode'] == 'NODE_MODE_REMOTE'}
        require(nodes == {cluster.nodes[name]['node_id'] for name in expected}, f'{user} node visibility is incorrect: {nodes}')
    denied(cluster.manage('bob', 'GetNode', {'node_id': cluster.nodes['alice-a']['node_id']}), 'RESOURCE_NOT_FOUND')
    denied(cluster.rpc('AgentNodeManagement', 'GetNode', {'node_id': cluster.nodes['alice-a']['node_id']}), 'UNAUTHENTICATED')
    denied(cluster.rpc('AgentNodeControl', 'Register', {
        'auth': {'node_id': cluster.nodes['alice-a']['node_id'], 'token': 'invalid-credential'}, 'params': {}}), 'UNAUTHENTICATED')
    try:
        cluster.rpc('AgentHarnessRpc', 'ClearSession', {
            'auth': {'node_id': cluster.nodes['alice-a']['node_id'], 'token': 'invalid-lease'},
            'params': {}}, channel=cluster.make_channel(cluster.nodes['alice-a']['port']))
    except grpc.RpcError as error:
        require(error.code() == grpc.StatusCode.UNAUTHENTICATED, 'Wrong Harness auth failure')
    else:
        raise AssertionError('Unauthenticated Harness request was accepted')
    passed('registration_api_owner_and_auth')

    before = cluster.info('alice-a')
    wait_for(lambda: int(cluster.info('alice-a')['lease_expires_at']) > int(before['lease_expires_at']), 'lease renewal', 40)
    require(cluster.info('bob-a').get('available'), 'Other node lost availability during renewal')
    passed('independent_node_lease_renewal')

    cluster.stop(cluster.nodes['alice-a']['process'])
    require(not cluster.info('alice-a').get('available', False), 'Stopped Worker still available')
    cluster.start_worker('alice-a')
    wait_for(lambda: cluster.info('alice-a').get('lease_expires_at'), 'registration before first heartbeat')
    node = cluster.nodes['alice-a']
    renamed = cluster.manage('alice', 'UpdateNode', {
        'node_id': node['node_id'], 'display_name': 'Renamed laptop',
        'endpoint': f'https://127.0.0.1:{node["port"]}', 'status': 'NODE_STATUS_ENABLED'})['node']
    require(not renamed.get('available'), 'Rename scenario missed the pre-heartbeat window')
    require(renamed['node_id'] == before['node_id'], 'Rename changed node identity')
    cluster.healthy_all()
    marker_turn(cluster, 'alice', 'alice-a')
    passed('rename_before_heartbeat_keeps_uuid_and_execution')
    after = cluster.info('alice-a')
    require(after['created_at'] == before['created_at'], 'Worker restart created another identity')
    require(cluster.query("SELECT count(*) FROM agent_nodes WHERE mode='remote'") == [(3,)], 'Node restart duplicated records')
    passed('worker_restart_preserves_identity')

    cluster.stop(cluster.server)
    cluster.start_server()
    cluster.healthy_all()
    require(cluster.info('alice-a')['created_at'] == before['created_at'], 'Server restart lost node identity')
    passed('server_restart_reregistration')

    node = cluster.nodes['alice-a']
    endpoint = f'https://127.0.0.1:{node["port"]}'
    marker = uuid.uuid4().hex
    started_file, forbidden_file = f'started-{marker}.txt', f'forbidden-{marker}.txt'
    session, response = cluster.start_turn('alice',
        f"Use Bash once to run exactly: printf '{marker}' > {started_file}; sleep 25; printf '{marker}' > {forbidden_file} . Do not run any other tool.")
    data(response)
    wait_for(lambda: list(node['home'].rglob(started_file)), 'running tool before node disable', 45)
    side_effect_deadline = time.monotonic() + 27
    disabled = cluster.manage('alice', 'UpdateNode', {'node_id': cluster.nodes['alice-a']['node_id'], 'display_name': 'alice-a',
                                                     'endpoint': endpoint, 'status': 'NODE_STATUS_DISABLED'})
    require('node' in disabled and not disabled['node'].get('available'), 'Disable did not withdraw node')
    # The Worker must stop on the control-plane rejection itself. Killing it
    # here would conceal the missing lease-observer notification.
    wait_for(lambda: node['process'].poll() is not None, 'revoked Worker cleanup', 18)
    while time.monotonic() < side_effect_deadline:
        require(not list(node['home'].rglob(forbidden_file)), 'Revoked execution produced a delayed side effect')
        time.sleep(0.25)
    passed('authentication_rejection_stops_running_tool')
    old_token = node['credential'].read_text()
    rotated = cluster.manage('alice', 'RotateCredential', {'node_id': cluster.nodes['alice-a']['node_id'], 'expires_at': int(time.time()) + 3600})
    require('credential' in rotated, 'RotateCredential failed')
    enabled = cluster.manage('alice', 'UpdateNode', {'node_id': cluster.nodes['alice-a']['node_id'], 'display_name': 'alice-a',
                                                    'endpoint': endpoint, 'status': 'NODE_STATUS_ENABLED'})
    require('node' in enabled, 'Enable failed')
    denied(cluster.rpc('AgentNodeControl', 'Register', {
        'auth': {'node_id': cluster.nodes['alice-a']['node_id'], 'token': old_token}, 'params': {}}), 'UNAUTHENTICATED')
    node['credential'].write_text(rotated['credential']['token'])
    cluster.start_worker('alice-a')
    cluster.healthy_all()
    passed('disable_rotate_and_rejoin')


def marker_turn(cluster, user, name):
    marker = uuid.uuid4().hex
    filename = f'{name}-{marker}.txt'
    session, answer, events = cluster.turn(user,
        f"Use Bash once to run: printf '{marker}' > {filename} . Then reply exactly {marker}.",
        node_id=cluster.nodes[name]['node_id'])
    require(marker in answer, 'Model did not return the requested marker')
    cluster.verify_file(name, filename, marker)
    require(cluster.query('SELECT execution_node_id FROM sessions WHERE session_key=?', (session,)) == [(cluster.nodes[name]['node_id'],)], 'Session bound to wrong Worker')
    require(any(event['payload']['type'] == 'messageBlockDelta' for event in events), 'No streamed delta received')
    return session, filename, marker


def routing(cluster):
    with ThreadPoolExecutor(max_workers=2) as tasks:
        alice = tasks.submit(marker_turn, cluster, 'alice', 'alice-a')
        bob = tasks.submit(marker_turn, cluster, 'bob', 'bob-a')
        session, filename, marker = alice.result()
        bob.result()
    passed('concurrent_owner_isolation_and_live_events')
    _, answer, _ = cluster.turn('alice', f'Read {filename} using Bash. Reply with its content and your previous final answer.', session)
    require(answer.count(marker) >= 2, 'Second Turn did not preserve file and history')
    history = data(http(f'{cluster.url}/v1/sessions/{session}/history', token=cluster.users['alice']['access_token']))
    require(len(history['data']) >= 2, 'Control-plane history missing completed Turns')
    passed('same_node_history_and_workspace')

    # Abrupt exit does not get to send Unregister; heartbeat expiry must remove
    # eligibility while preserving the existing Session's node binding.
    cluster.nodes['alice-a']['process'].kill()
    cluster.nodes['alice-a']['process'].wait(timeout=5)
    wait_for(lambda: not cluster.info('alice-a').get('available'), 'crashed Worker heartbeat expiry', 40)
    _, response = cluster.start_turn('alice', 'Reply exactly MUST_NOT_RUN', session)
    require(response.get('error', {}).get('code') == 'serviceUnavailable', f'Pinned Session silently moved: {response}')
    marker_turn(cluster, 'alice', 'alice-b')
    passed('pinned_failure_and_new_session_routing')
    cluster.start_worker('alice-a')
    cluster.healthy_all()
    _, answer, _ = cluster.turn('alice', f'Read {filename} and reply with the content.', session)
    require(marker in answer, 'Worker restart lost the existing Workspace')
    passed('worker_restart_existing_session')

    cluster.stop(cluster.server)
    cluster.start_server()
    cluster.healthy_all()
    _, answer, _ = cluster.turn('alice', f'Read {filename} and reply with the content.', session)
    require(marker in answer, 'Healthy registration after Server restart was not executable')
    passed('server_restart_existing_session_execution')


def reliability(cluster):
    marker = uuid.uuid4().hex
    filename = f'long-{marker}.txt'
    before = int(cluster.info('alice-a')['lease_expires_at'])
    session = f'sess_{uuid.uuid4().hex}'
    with ThreadPoolExecutor(max_workers=2) as tasks:
        long_turn = tasks.submit(cluster.turn, 'alice',
            f"Use Bash once with command: printf 'started'; sleep 40; printf '{marker}' > {filename} . "
            f'Do not return until it completes. Then reply exactly {marker}.', session, timeout=90, node_id=cluster.nodes['alice-a']['node_id'])
        other = tasks.submit(marker_turn, cluster, 'bob', 'bob-a')
        def has_live_tool_call():
            if long_turn.done():
                long_turn.result()
            for path in REPORT.glob(f'{session}_*.jsonl'):
                # The SSE writer may be in the middle of its next record.
                records = [json.loads(line)['record'] for line in path.read_text().split('\n')[:-1]]
                payloads = [record['event']['data']['payload']['type'] for record in records
                            if record.get('event', {}).get('type') == 'agent']
                if 'messageBlockStarted' in payloads and any(
                    record['event']['data']['payload'].get('data', {}).get('block', {}).get('content', {}).get('type') == 'toolCall'
                    for record in records if record.get('event', {}).get('type') == 'agent'):
                    require('turnCompleted' not in payloads, 'Tool progress observed only after completion')
                    return True
            return False
        wait_for(has_live_tool_call, 'live tool call while execution is pending', 60)
        require(not list(cluster.nodes['alice-a']['home'].rglob(filename)), 'Tool finished before streaming observation')
        passed('progress_arrives_before_worker_side_effect')
        other.result()
        passed('other_worker_executes_during_long_turn')
        session, answer, _ = long_turn.result()
    require(int(cluster.info('alice-a')['lease_expires_at']) > before, 'Scenario did not cross a lease renewal')
    require(marker in answer, 'Long Turn did not complete successfully')
    cluster.verify_file('alice-a', filename, marker)
    require(not cluster.query('SELECT turn_id FROM agent_active_turns WHERE session_id=?', (session,)), 'Long Turn left an active record')
    passed('long_turn_survives_renewal_and_settles')


def collaboration(cluster):
    marker = uuid.uuid4().hex
    filename = f'child-{marker}.txt'
    session, answer, events = cluster.turn('alice',
        'Use spawn_agent to create exactly one child named writer. Delegate this exact task: '
        f'Use Bash to run sleep 15; printf "{marker}" > {filename}, then reply {marker}. '
        'Immediately use wait_agent with target writer and timeoutMs 60000, without doing other work first. '
        f'After its successful completion, use Bash to read {filename} and reply {marker}. Do not perform the child task yourself.',
        timeout=90)
    require(marker in answer, 'Parent did not receive child result')
    children = cluster.query('SELECT session_key, execution_node_id FROM sessions WHERE parent_session_id=?', (session,))
    require(len(children) == 1 and children[0][1] == cluster.nodes['alice-a']['node_id'], f'Child node binding incorrect: {children}')
    results = [block['content']['data'] for event in events if event['payload']['type'] == 'messageCompleted'
               for block in event['payload']['data']['blocks'] if block['content']['type'] == 'toolResult']
    waits = [result for result in results if result['toolName'] == 'wait_agent']
    require(waits, 'Real model did not exercise wait_agent; collaboration not verified')
    require(all(not result['isError'] for result in waits), 'Remote wait_agent failed; inspect event log')
    wait_calls = [block for event in events if event['payload']['type'] == 'messageCompleted'
                  for block in event['payload']['data']['blocks']
                  if block['content']['type'] == 'toolCall' and block['content']['data']['name'] == 'wait_agent']
    require(any(block['content']['data']['arguments'].get('value', {}).get('timeoutMs') == 60000
                and block['completedAt'] - block['startedAt'] >= 10 for block in wait_calls),
            'Model did not exercise a pending wait exceeding 10s; long wait not verified')
    child_events = [json.loads(row[0]) for row in cluster.query(
        'SELECT payload_json FROM agent_events WHERE session_id=? ORDER BY event_seq', (children[0][0],))]
    require(child_events and child_events[-1]['type'] == 'turnCompleted', 'Child did not persist a successful terminal event')
    cluster.verify_file('alice-a', filename, marker)
    passed('real_child_wait_result_and_node_affinity')


def workspace(cluster):
    marker = 'preference_' + uuid.uuid4().hex
    session, _, events = cluster.turn('alice',
        f'For all our future work, remember that my required report identifier is {marker}. '
        'This is a permanent collaboration preference. Do not call any tools in this Turn. Just acknowledge it.')
    tool_results = [block['content']['data'] for event in events if event['payload']['type'] == 'messageCompleted'
                    for block in event['payload']['data']['blocks'] if block['content']['type'] == 'toolResult']
    require(not tool_results, 'Model called a tool despite the scenario instruction; automatic reflection not isolated')
    user_id = cluster.users['alice']['user']['user_id']
    server_workspace = cluster.server_home / 'workspace-talos' / user_id
    node_workspace = cluster.nodes['alice-a']['home'] / 'workspace-talos' / user_id
    artifacts = [json.loads(line) for path in (node_workspace / 'sessions' / session).glob('*.jsonl')
                 for line in path.read_text().splitlines() if line.strip()]
    require(any(record.get('kind') == 'transcript' for record in artifacts), f'Worker transcript missing for {session}')
    passed('remote_transcript_persisted_on_worker')
    unexpected = [str(path.relative_to(server_workspace)) for path in server_workspace.rglob('*') if path.is_file()]
    require(not unexpected, f'Remote Workspace files were created on control plane: {unexpected}')
    memories = list(node_workspace.rglob('MEMORY.md'))
    require(any(marker in path.read_text() for path in memories), 'Automatic reflection did not persist MEMORY.md on Worker')
    _, answer, _ = cluster.turn('alice', 'What is my required report identifier? Reply only with the identifier.')
    require(marker in answer, 'New remote Session cannot read Worker Memory')
    wait_for(lambda: cluster.query('SELECT memory_processed_until_event_seq FROM sessions WHERE session_key=?', (session,))[0][0] > 0,
             'scheduled Dream Memory commits on bound Worker', 60)
    dream_files = [path for path in node_workspace.rglob('*.md') if path.name != 'MEMORY.md']
    require(any(marker in path.read_text() for path in dream_files), 'Dream Memory content was not persisted on Worker')
    usage = cluster.query('SELECT DISTINCT usage_kind FROM profile_turn_usage WHERE session_id=?', (session,))
    require({('memory',), ('dream',)} <= set(usage), f'Memory model usage not accounted on control plane: {usage}')
    unexpected = [str(path.relative_to(server_workspace)) for path in server_workspace.rglob('*') if path.is_file()]
    require(not unexpected, f'Maintenance created remote Workspace files on control plane: {unexpected}')
    passed('scheduled_dream_memory_on_worker_with_control_plane_usage')
    passed('workspace_memory_and_artifacts_stay_on_worker')


    completed_at = cluster.query('SELECT occurred_at FROM agent_events WHERE session_id=? ORDER BY event_seq DESC LIMIT 1',
                                 (session,))[0][0]
    node_date = datetime.fromtimestamp(completed_at, ZoneInfo('Pacific/Kiritimati')).strftime('%Y-%m-%d')
    control_date = datetime.fromtimestamp(completed_at, ZoneInfo('Pacific/Honolulu')).strftime('%Y-%m-%d')
    require(node_date != control_date, 'Timezone fixture did not cross a date boundary')
    require((node_workspace / 'memory' / f'{node_date}.md').is_file(), 'Dream Memory did not use Worker-local date')
    require(not (node_workspace / 'memory' / f'{control_date}.md').exists(), 'Dream Memory used control-plane date')
    passed('worker_policy_and_date_are_independent_of_control_plane_timezone')


def rpc_boundaries(cluster):
    first, _, _ = marker_turn(cluster, 'alice', 'alice-a')
    worker = cluster.nodes['alice-a']['process']
    # The existing Service has a process-wide execution gate for close. Exercise
    # the methods separately so the second request actually reaches a live node.
    for operation in ('compact', 'close'):
        worker.send_signal(signal.SIGSTOP)
        try:
            started = time.monotonic()
            response = http(f'{cluster.url}/v1/sessions/{first}/{operation}', {}, cluster.users['alice']['access_token'], 45)
            require(response.get('error', {}).get('code') == 'timeout', f'{operation} did not return a typed timeout: {response}')
            require(time.monotonic() - started < 40, f'{operation} deadline did not bound the call')
            require(not cluster.query('SELECT turn_id FROM agent_active_turns WHERE session_id=?', (first,)),
                    'Uncommitted admission timeout left an active Turn')
        finally:
            worker.send_signal(signal.SIGCONT)
        cluster.healthy_all()
    passed('nonresponsive_worker_unary_and_admission_are_bounded')

    marker = uuid.uuid4().hex
    started_file, forbidden_file = f'shutdown-start-{marker}.txt', f'shutdown-forbidden-{marker}.txt'
    session, response = cluster.start_turn('alice',
        f"Use Bash once to run exactly: printf '{marker}' > {started_file}; sleep 5; printf '{marker}' > {forbidden_file} . Do not use other tools.")
    data(response)
    wait_for(lambda: list(cluster.nodes['alice-a']['home'].rglob(started_file)), 'running tool before graceful shutdown', 45)
    cluster.server.send_signal(signal.SIGSTOP)
    try:
        worker.terminate()
        # Unregister cannot reach the paused control plane. Local revocation must
        # nevertheless stop Bash before its delayed file write.
        wait_for(lambda: worker.poll() is not None, 'Worker shutdown with unavailable control plane', 25)
        require(not list(cluster.nodes['alice-a']['home'].rglob(forbidden_file)), 'Shutdown waited for Unregister before cancelling execution')
    finally:
        cluster.server.send_signal(signal.SIGCONT)
    completed = cluster.query("SELECT count(*) FROM agent_events WHERE session_id=? AND json_extract(payload_json, '$.type')='turnCompleted'", (session,))
    require(completed == [(0,)], 'Interrupted transport fabricated completion')
    passed('graceful_shutdown_revokes_locally_before_unregister')


def lease_expiry(cluster):
    expiry = max(int(cluster.info(name)['lease_expires_at']) for name in cluster.nodes)
    cluster.server.send_signal(signal.SIGSTOP)
    try:
        deadline = time.monotonic() + max(0, expiry - time.time()) + 3
        while time.monotonic() < deadline:
            require(all(node['process'].poll() is None for node in cluster.nodes.values()),
                    'Worker exited while control plane was unavailable')
            time.sleep(0.5)
    finally:
        cluster.server.send_signal(signal.SIGCONT)
    cluster.healthy_all()
    require(all(int(cluster.info(name)['lease_expires_at']) > expiry for name in cluster.nodes),
            'Workers did not acquire new leases after the outage')
    passed('expired_inflight_control_rpc_reregisters_without_worker_exit')


def parity(cluster):
    token = cluster.users['alice']['access_token']
    models = data(http(cluster.url + '/v1/models?nodeId=' + cluster.nodes['alice-a']['node_id'], token=token))['models']
    image_model = os.environ.get('TALOS_E2E_IMAGE_MODEL', 'deepseek-v4-flash-vision-exp')
    cluster.model = next(model for model in models if model['id'] == image_model)
    marker = 'attached_' + uuid.uuid4().hex
    attachment = ROOT / 'build/desktop/icons/icon.png'
    upload = subprocess.run(['curl', '-sS', '--noproxy', '*', '-X', 'POST', cluster.url + '/v1/files',
                             '-H', f'Authorization: Bearer {token}',
                             '-F', 'metadata={"filename":"attached.png","scope":{"type":"personal"},"tags":[],"metadata":{}};type=application/json',
                             '-F', f'file=@{attachment};type=image/png'], check=False, capture_output=True)
    require(upload.returncode == 0, f'Attachment upload transport failed: curl exit {upload.returncode}')
    file_id = data(json.loads(upload.stdout))['file']['fileId']
    session, response = cluster.start_turn('alice', f'Describe the dominant non-white color in the attached image, in English. Remember this session identifier: {marker}. Include it in your answer.', file_ids=[file_id])
    events = cluster.events('alice', session, data(response)['turnId'], 90)
    require(events[-1]['payload']['type'] == 'turnCompleted', 'Attached input failed remotely')
    require('blue' in final_answer(events).lower() and marker in final_answer(events), 'Worker did not receive and describe the attached image')
    passed('attachment_resolved_through_authorized_control_plane_port')

    data(http(f'{cluster.url}/v1/sessions/{session}/compact', {}, token, timeout=90))
    workspace = cluster.nodes['alice-a']['home'] / 'workspace-talos' / cluster.users['alice']['user']['user_id']
    artifacts = [json.loads(line) for path in (workspace / 'sessions' / session).glob('*.jsonl')
                 for line in path.read_text().splitlines() if line.strip()]
    require(any(record.get('kind') == 'summary' for record in artifacts), 'Explicit compaction produced no Worker summary')
    _, answer, _ = cluster.turn('alice', 'What was the session identifier given with the attachment? Reply with the identifier only.', session)
    require(marker in answer, 'Compacted remote Session lost context')
    passed('explicit_remote_compaction_preserves_context_and_worker_summary')

    session, response = cluster.start_turn('alice',
        'Call AskUserQuestion exactly once. Use header Execution Mode, question Choose mode, '
        'and exactly these options: id plan, label Plan, description Explore before execution; '
        'id direct, label Direct, description Execute immediately. '
        'After receiving the answer, reply QUESTION_RESUMED_OK.', session)
    turn_id = data(response)['turnId']
    waiting = cluster.events('alice', session, turn_id, 90)
    require(waiting[-1]['payload']['type'] == 'waitingForInput', 'Remote question did not enter waiting state')
    action = waiting[-1]['payload']['data']['action']
    require(action['params']['type'] == 'question', 'Expected a typed question action')
    require(any((workspace / 'sessions' / session).rglob('*')), 'Worker has no continuation artifacts')
    records = [json.loads(line)['record'] for line in (REPORT / f'{session}_{turn_id}.jsonl').read_text().splitlines()]
    cursor = records[-1]['eventId']
    # Prove durable continuation survives a real Worker process restart.
    cluster.stop(cluster.nodes['alice-a']['process'])
    cluster.start_worker('alice-a')
    cluster.healthy_all()
    resumed = data(http(cluster.url + '/v1/turns/input', {
        'identity': {'sessionId': session, 'turnId': turn_id},
        'input': {'type': 'userActionResponse', 'actionId': action['actionId'],
                  'response': {'type': 'question', 'response': {'type': 'selectedOption', 'optionId': 'plan'}}},
        'clientRequestId': None}, token))
    require(resumed['identity']['turnId'] == turn_id, 'User action started a different logical Turn')
    events = cluster.events('alice', session, turn_id, 90, after=cursor)
    require(events[-1]['payload']['type'] == 'turnCompleted' and 'QUESTION_RESUMED_OK' in final_answer(events),
            'Remote continuation did not resume after Worker restart')
    require(not cluster.query('SELECT turn_id FROM agent_active_turns WHERE session_id=?', (session,)), 'Resumed Turn left active state')
    passed('remote_question_continuation_survives_worker_restart')
    server_workspace = cluster.server_home / 'workspace-talos' / cluster.users['alice']['user']['user_id']
    require(not any(path.is_file() for path in server_workspace.rglob('*')), 'Compaction or continuation wrote control-plane Workspace files')

    embedded_marker = uuid.uuid4().hex
    embedded_file = f'embedded-{embedded_marker}.txt'
    cluster.enroll_management_host('admin')
    passed('management_host_enrolled_through_public_sign_in_bootstrap')
    embedded_session, _, _ = cluster.turn('admin', f"Use Bash to run printf '{embedded_marker}' > {embedded_file}, then acknowledge.", node_id='embedded')
    require(cluster.query('SELECT execution_node_id FROM sessions WHERE session_key=?', (embedded_session,)) == [('embedded',)],
            'User without remote nodes did not retain embedded routing')
    local_files = list(cluster.server_home.rglob(embedded_file))
    require(len(local_files) == 1 and local_files[0].read_text() == embedded_marker, 'Embedded Worker did not execute locally')
    require(not any(list(node['home'].rglob(embedded_file)) for node in cluster.nodes.values()), 'Embedded execution leaked to remote nodes')
    passed('same_worker_library_preserves_embedded_execution')


def final_answer(events):
    return ''.join(block['content']['data']['text'] for event in events
                   if event['payload']['type'] == 'messageCompleted'
                   for block in event['payload']['data']['blocks']
                   if block['content']['type'] == 'assistantText' and block['content']['data']['phase'] == 'finalAnswer')


if __name__ == '__main__':
    scenario = sys.argv[1]
    cluster = None
    outcome = 'FAIL'
    try:
        cluster = Cluster(dream_maintenance=scenario == 'workspace')
        cluster.start()
        passed('three_real_workers_created_through_public_api')
        {'registration': registration, 'routing': routing, 'reliability': reliability,
         'collaboration': collaboration, 'workspace': workspace, 'parity': parity, 'lease_expiry': lease_expiry, 'rpc_boundaries': rpc_boundaries}[scenario](cluster)
        outcome = 'PASS'
    except Exception as error:
        print(f'DETAIL {type(error).__name__}: {error}', flush=True)
        traceback.print_exc()
    finally:
        if cluster is not None:
            cluster.close()
    print(f'CASE multi_node_{scenario} {outcome}', flush=True)
    (REPORT / 'result.json').write_text(json.dumps({'scenario': scenario, 'status': outcome}) + '\n')
    sys.exit(0 if outcome == 'PASS' else 1)
