"""Actual configuration commits, publication and per-Computer admission."""
from copy import deepcopy
from pathlib import Path
from urllib.parse import urlencode
import uuid
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import yaml
from fixture import data, http, require, wait_for
from scenarios import passed
from environment import memory_document


def configuration(cluster):
    token = cluster.users['alice']['access_token']
    a, b = [cluster.nodes[name] for name in ('alice-a', 'alice-b')]
    qa, qb = [urlencode({'nodeId': node['node_id']}) for node in (a, b)]
    original = data(http(f'{cluster.url}/v1/config/llm?{qa}', token=token))
    require(original['type'] == 'llm', 'Wrong configuration projection')
    bob_query = urlencode({'nodeId': cluster.nodes['bob-a']['node_id']})
    bob_config = data(http(f'{cluster.url}/v1/config/llm?{bob_query}', token=cluster.users['bob']['access_token']))
    require(bob_config['type'] == 'llm', 'Bob could not read his Computer configuration')
    passed('each_owner_manages_personal_worker_configuration')

    before_server = Path(cluster.server_env['TALOS_CONFIG']).read_bytes()
    marker = 'MODEL_' + uuid.uuid4().hex
    value = deepcopy(original['config'])
    secret = 'unused-provider-' + uuid.uuid4().hex
    value['openai']['api_key'] = secret
    provider = value[cluster.model['provider']]
    next(model for model in provider['models'] if model['id'] == cluster.model['id'])['name'] = marker
    updated = data(http(f'{cluster.url}/v1/config?{qa}', {'type': 'llm', 'value': value}, token, method='PUT'))
    expected = deepcopy(value)
    expected['openai']['api_key'] = updated['config']['openai'].get('api_key')
    require(updated['config'] == expected, 'Configuration RPC changed canonical fields')
    require(secret not in json.dumps(updated), 'Configuration read exposed the stored provider secret')
    require(secret in (a['home'] / '.env').read_text(), 'Provider secret was not saved on selected Worker')
    require(secret not in (cluster.server_home / '.env').read_text(), 'Provider secret was copied to control plane')
    catalog = data(http(f'{cluster.url}/v1/models?{qa}', token=token))
    require(any(model['name'] == marker for model in catalog['models']), 'Committed model metadata was not published on Worker')
    persisted = yaml.safe_load(Path(a['env']['TALOS_CONFIG']).read_text())
    require(any(model['name'] == marker for model in persisted['llm'][cluster.model['provider']]['models']), 'Configuration was not persisted on selected Worker')
    require(Path(cluster.server_env['TALOS_CONFIG']).read_bytes() == before_server, 'Control-plane configuration was modified')
    require(not any(model['name'] == marker for model in data(http(f'{cluster.url}/v1/models?{qb}', token=token))['models']), 'Configuration leaked to another Worker')
    passed('config_commit_and_runtime_publication_are_node_local')

    search = data(http(f'{cluster.url}/v1/config/webSearch?{qa}', token=token))['config']
    config_path = Path(a['env']['TALOS_CONFIG'])
    before = config_path.read_bytes()
    invalid = deepcopy(search)
    invalid['brave']['timeout_ms'] = 0
    failure = http(f'{cluster.url}/v1/config?{qa}', {'type': 'webSearch', 'value': invalid}, token, method='PUT')
    require(failure.get('error', {}).get('code') == 'invalidRequest', 'Invalid candidate was accepted')
    require(config_path.read_bytes() == before, 'Failed candidate changed persisted configuration')
    require(data(http(f'{cluster.url}/v1/config/webSearch?{qa}', token=token))['config'] == search, 'Failed candidate replaced runtime configuration')
    passed('failed_candidate_does_not_persist_or_publish')

    signal = uuid.uuid4().hex
    workspace = data(http(f'{cluster.url}/v1/workspace/info?{qa}', token=token))
    started = Path(workspace['userWorkspaceDir']) / ('active-' + signal)
    session, ack = cluster.start_turn('alice', f'Use Bash once to run: touch {started}; sleep 25; echo CONFIG_GATE_OK. Then reply CONFIG_GATE_OK.', node_id=a['node_id'])
    ack = data(ack)
    wait_for(lambda: started.exists(), 'Actual Worker tool execution started', seconds=60)
    failure = http(f'{cluster.url}/v1/config?{qa}', {'type': 'webSearch', 'value': search}, token, method='PUT')
    require(failure.get('error', {}).get('code') == 'conflict', 'Running node accepted configuration commit')
    require(config_path.read_bytes() == before, 'Rejected active-node commit changed its file')
    other = data(http(f'{cluster.url}/v1/config/webSearch?{qb}', token=token))['config']
    data(http(f'{cluster.url}/v1/config?{qb}', {'type': 'webSearch', 'value': other}, token, method='PUT'))
    other_session, other_ack = cluster.start_turn('alice', 'Reply CONFIG_OTHER_NODE_OK.', node_id=b['node_id'])
    other_ack = data(other_ack)
    require(cluster.query('SELECT state FROM agent_active_turns WHERE session_id=?', (session,)), 'Concurrency fixture did not overlap active execution')
    other_events = cluster.events('alice', other_session, other_ack['turnId'], 90)
    require(other_events[-1]['payload']['type'] == 'turnCompleted', 'Other Computer could not execute')
    events = cluster.events('alice', session, ack['turnId'], 90)
    require(events[-1]['payload']['type'] == 'turnCompleted', 'Active execution did not settle')
    data(http(f'{cluster.url}/v1/config/reload?{qa}', {}, token))
    passed('active_node_rejects_reload_while_other_node_commits_and_executes')

    # A controlled search endpoint proves the real Provider consumes the new Worker runtime.
    hits = []
    search_key = 'search-fixture-' + uuid.uuid4().hex
    class SearchHandler(BaseHTTPRequestHandler):
        def do_GET(self):
            hits.append(self.headers.get('x-subscription-token'))
            response = json.dumps({'web': {'results': [{'url': 'https://example.com/evidence', 'title': 'CONFIG_SEARCH_RELOADED', 'description': 'Actual target Worker runtime evidence.'}]}}).encode()
            self.send_response(200)
            self.send_header('content-type', 'application/json')
            self.send_header('content-length', str(len(response)))
            self.end_headers()
            self.wfile.write(response)
        def log_message(self, *_):
            pass
    endpoint = ThreadingHTTPServer(('127.0.0.1', 0), SearchHandler)
    thread = threading.Thread(target=endpoint.serve_forever, daemon=True)
    thread.start()
    try:
        changed = deepcopy(search)
        changed['provider'] = 'brave'
        changed['brave'].update(base_url=f'http://127.0.0.1:{endpoint.server_port}', api_key=search_key)
        data(http(f'{cluster.url}/v1/config?{qa}', {'type': 'webSearch', 'value': changed}, token, method='PUT'))
        _, answer, _ = cluster.turn('alice', 'Use web_search once with query "configuration reload evidence" and limit 1. Report the exact result title. Do not use Bash or another tool.', session)
        require(hits == [search_key] and 'CONFIG_SEARCH_RELOADED' in answer, 'Existing Harness Session did not consume the reloaded Web Search configuration')
        require(search_key not in json.dumps(data(http(f'{cluster.url}/v1/config/webSearch?{qa}', token=token))), 'Web Search read exposed its credential')
        passed('existing_harness_uses_reloaded_execution_config_and_worker_secret')
    finally:
        endpoint.shutdown()
        endpoint.server_close()
        thread.join(timeout=3)

    # Reload a new Workspace definition through the same port; adapter must observe live config.
    saved = yaml.safe_load(config_path.read_text())
    saved['workspaces']['default_workspace_id'] = 'reloaded'
    saved['workspaces']['definitions'].append({'workspace_id': 'reloaded', 'root_prefix': 'reloaded-files'})
    config_path.write_text(yaml.safe_dump(saved, sort_keys=False))
    data(http(f'{cluster.url}/v1/config/reload?{qa}', {}, token))
    info = data(http(f'{cluster.url}/v1/workspace/info?{qa}', token=token))
    require(info['workspaceId'] == 'reloaded', 'Worker default did not reload')
    fresh, _, _ = cluster.turn('alice', 'Reply RELOADED_WORKSPACE_OK.', node_id=a['node_id'])
    require(cluster.query('SELECT workspace_id FROM sessions WHERE session_key=?', (fresh,)) == [('reloaded',)], 'Harness adapter retained startup Workspace configuration')
    passed('reloaded_workspace_definition_reaches_real_harness')
    memory_id = 'mem_' + uuid.uuid4().hex
    memory_marker = 'RELOAD_MEMORY_' + uuid.uuid4().hex
    content = memory_document(info['workspaceId'], memory_id, memory_marker + ' reload-needle')
    data(http(f'{cluster.url}/v1/memory/files/2026-10-11', {'nodeId': a['node_id'], 'date': '2026-10-11', 'content': content, 'expected_version': None}, token, method='PUT'))
    _, answer, events = cluster.turn('alice', 'Use MemorySearch to search reload-needle, then MemoryGet to read the returned memory ID. Reply with the marker from that memory body. Do not use Bash or Read.', fresh)
    require(memory_marker in answer, 'Retained Memory tools did not use the reloaded Workspace/index')
    results = [event['payload']['data']['block']['content']['data'] for event in events
               if event['payload']['type'] == 'messageBlockCompleted' and event['payload']['data']['block']['content']['type'] == 'toolResult']
    require({'MemorySearch', 'MemoryGet'} <= {result['toolName'] for result in results if not result['isError']}, 'Reload acceptance did not execute successful Memory tools')
    data(http(f'{cluster.url}/v1/sessions/{fresh}/compact', {}, token, timeout=90))
    _, answer, _ = cluster.turn('alice', 'What marker did you retrieve from Memory? Reply with only that marker.', fresh)
    require(memory_marker in answer, 'Compaction after Workspace reload lost Memory context')
    require(memory_marker in (Path(info['userWorkspaceDir']) / 'memory/2026-10-11.md').read_text(), 'Reloaded Memory file was not Worker-local')
    passed('retained_memory_tools_and_compaction_follow_reloaded_workspace')


    cluster.stop(a['process'])
    cluster.start_worker('alice-a')
    wait_for(lambda: cluster.info('alice-a').get('available'), 'Configuration Worker restarted')
    catalog = data(http(f'{cluster.url}/v1/models?{qa}', token=token))
    require(any(model['name'] == marker for model in catalog['models']), 'Committed configuration was lost on restart')
    passed('same_uuid_restart_preserves_worker_configuration')
