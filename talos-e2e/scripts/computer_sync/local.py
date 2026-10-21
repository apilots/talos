"""Unregistered DEV host remains a complete local product without G or H."""
import json
import os
from pathlib import Path
import sys
import uuid
import yaml

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'multi_node'))
from fixture import Cluster, ROOT, REPORT, data, http, require, wait_for

cluster = Cluster()
try:
    env = cluster.config(cluster.server_home, {'worker_rpc': {'bind_address': '127.0.0.1:0'}})
    path = Path(env['TALOS_CONFIG'])
    config = yaml.safe_load(path.read_text())
    config['gateway']['auth'] = False
    config['sync'] = {'enabled': False, 'hub_endpoint': f'wss://127.0.0.1:{cluster.rpc_port}/v1/sync'}
    path.write_text(yaml.safe_dump(config, sort_keys=False))
    env['HOME'] = str(cluster.server_home)
    def start():
        process = cluster.start_process('local-server', [os.environ.get('TALOS_E2E_SERVER_BIN', str(ROOT / 'talos-rs/target/debug/talos-server'))], env)
        wait_for(lambda: http(cluster.url + '/health'), 'Unregistered local Server startup')
        return process
    server = start()
    status = data(http(cluster.url + '/v1/sync/status'))
    require(status['localComputerId'] == 'embedded' and status['configuredEnabled'] is False, 'Local Server identity/sync opt-out')
    computers = data(http(cluster.url + '/v1/computers'))['data']
    require(len(computers) == 1 and computers[0]['nodeId'] == 'embedded' and computers[0]['available'], 'Unregistered embedded Worker unavailable')
    models = data(http(cluster.url + '/v1/models'))['models']
    wanted = os.environ.get('TALOS_E2E_PREFERRED_MODELS', 'deepseek-v4-flash').split(',')
    model = next((m for identifier in wanted for m in models if m['id'] == identifier), models[0])
    session = str(uuid.uuid4())
    marker = 'LOCAL_NO_CLOUD_' + uuid.uuid4().hex
    filename = 'local-proof-' + uuid.uuid4().hex + '.txt'
    def history():
        return data(http(cluster.url + '/v1/sessions/' + session + '/history'))['data']
    def run(prompt):
        ack = data(http(cluster.url + '/v1/turns/start', {'nodeId': 'embedded', 'sessionId': session,
            'input': {'content': [{'type': 'text', 'text': prompt}]},
            'modelSelection': {'modelRef': {'provider': model['provider'], 'id': model['id']}, 'reasoningEffort': None},
            'userPermissionMode': 'fullAccess'}))
        def completed():
            turns = history()
            terminal = next((t['result'] for t in turns if t['identity']['turnId'] == ack['turnId']), None)
            if terminal is None:
                return False
            require(terminal['status'] == 'completed', 'Local Worker task failed')
            return turns
        return wait_for(completed, 'Local real model task', seconds=120)
    first = run(f'Use Bash once to write exactly {marker} into {filename} in the current workspace. Then reply exactly {marker}.')
    files = list(cluster.server_home.rglob(filename))
    require(len(files) == 1 and files[0].read_text().strip() == marker, 'Local Worker file missing')
    cluster.stop(server)
    server = start()
    require(history() == first, 'Unregistered local Server lost history on restart')
    complete = run(f'Use Bash to read {filename}. Reply with its contents and append LOCAL_RESTART_OK.')
    require(len(complete) == 2 and marker in json.dumps(complete[-1]['result']), 'Local continuation lost Worker state')
    started = 'local-running-' + uuid.uuid4().hex
    forbidden = 'local-cancelled-' + uuid.uuid4().hex
    running = data(http(cluster.url + '/v1/turns/start', {'nodeId': 'embedded', 'sessionId': session,
        'input': {'content': [{'type': 'text', 'text': f'Use Bash once: touch {started}; sleep 30; touch {forbidden}. Do not call another tool.'}]},
        'modelSelection': {'modelRef': {'provider': model['provider'], 'id': model['id']}, 'reasoningEffort': None},
        'userPermissionMode': 'fullAccess'}))
    wait_for(lambda: list(cluster.server_home.rglob(started)), 'No-cloud real tool started', 90)
    identity = {'sessionId': session, 'turnId': running['turnId']}
    admitted = data(http(cluster.url + '/v1/turns/input', {'identity': identity,
        'input': {'type': 'message', 'input': {'content': [{'type': 'text', 'text': 'Local live input without registration or sync.'}]}},
        'clientRequestId': str(uuid.uuid4())}))
    require(admitted['identity'] == identity, 'No-cloud Input admitted to the wrong Turn')
    interrupted = data(http(cluster.url + '/v1/turns/interrupt', {'identity': identity}))
    require(interrupted['identity'] == identity and interrupted['outcome']['type'] == 'cancelled', 'No-cloud cancellation did not reach embedded Worker')
    wait_for(lambda: data(http(cluster.url + '/v1/sessions/' + session + '/active-turn'))['activeTurn'] is None, 'No-cloud cancellation cleanup')
    require(not list(cluster.server_home.rglob(forbidden)), 'Cancelled local tool produced a delayed effect')
    print('CASE sync_local_input_and_cancel_without_cloud PASS', flush=True)
    require(not (cluster.root / 'cloud.db').exists(), 'Single-machine scenario started Cloud Auth')
    (REPORT / 'local-history.json').write_text(json.dumps(complete, indent=2))
    print('CASE sync_local_cold_start_without_cloud_and_durable_restart PASS', flush=True)
finally:
    cluster.close()
