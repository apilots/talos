"""Owner-configured local and remote Computers through the original APIs."""
from copy import deepcopy
import os
from pathlib import Path
import uuid
import yaml
from cluster import data, http, require, wait_for


def run(cluster):
    # Directory lookup conceals unowned nodes; an already published environment
    # rejects the same account at Coordinator authorization. Both are typed refusals.
    for name in ('Desktop-A', 'Mac-Worker', 'Docker-Worker'):
        local = name in cluster.desktops
        node = cluster.desktops[name]['node'] if local else cluster.nodes[name]['node_id']
        home = cluster.desktops[name]['home'] if local else cluster.nodes[name]['home']
        original = cluster.desktop('Desktop-B', 'config/get', {'nodeId': node, 'path': 'llm'})
        if local:
            require(cluster.desktop(name, 'config/get', {'path': 'llm'}) == original,
                    'Owner local default configuration differs from remote access')
        before = {host: (cluster.desktops[host]['home'] / 'conf/talos.yaml').read_bytes()
                  for host in ('Desktop-A', 'Desktop-B') if host != name}
        before_g = Path(cluster.server_env['TALOS_CONFIG']).read_bytes()
        changed = deepcopy(original['config'])
        marker = 'OWNER_CONFIG_' + uuid.uuid4().hex
        next(model for model in changed[cluster.model['provider']]['models']
             if model['id'] == cluster.model['id'])['name'] = marker
        cluster.desktop('Desktop-B', 'config/update', {'nodeId': node, 'value': {'type': 'llm', 'value': changed}})
        catalog = cluster.desktop('Desktop-A', 'model/list', {'nodeId': node})['models']
        require(any(model['name'] == marker for model in catalog), 'Owner model change was not published on target Worker')
        saved = yaml.safe_load((home / 'conf/talos.yaml').read_text())
        require(any(model['name'] == marker for model in saved['llm'][cluster.model['provider']]['models']),
                'Owner model change was not saved on target Worker')
        search = cluster.desktop('Desktop-B', 'config/get', {'nodeId': node, 'path': 'webSearch'})['config']
        updated = deepcopy(search)
        secret = 'owner-search-fixture-' + uuid.uuid4().hex
        updated['provider'] = 'brave'
        updated['brave'].update(base_url='https://search.example.test', api_key=secret)
        cluster.desktop('Desktop-B', 'config/update', {'nodeId': node, 'value': {'type': 'webSearch', 'value': updated}})
        require(cluster.desktop('Desktop-A', 'config/get', {'nodeId': node, 'path': 'webSearch'})['config']['brave']['base_url'] == 'https://search.example.test',
                'Owner Web Search change was not returned through the same RPC API')
        require(secret in (home / '.env').read_text(), 'Owner Web Search credential was not saved on target Worker')
        for host, old in before.items():
            require(secret not in (cluster.desktops[host]['home'] / '.env').read_text(), 'Target credential leaked into another control plane')
            require((cluster.desktops[host]['home'] / 'conf/talos.yaml').read_bytes() == old,
                    'Another control plane persisted the target Worker configuration')
        require(Path(cluster.server_env['TALOS_CONFIG']).read_bytes() == before_g,
                'G persisted an owner Computer configuration')
        denied = http(cluster.url + '/v1/config/llm?nodeId=' + node, token=cluster.users['bob']['access_token'])
        require(denied.get('error', {}).get('code') in ('resourceNotFound', 'permissionDenied'), 'Another account configuration response: status=' + str(denied.get('status')) + ' code=' + str(denied.get('error', {}).get('code')))
        print('CASE sync_owner_configures_models_and_web_search_' + name.lower().replace('-', '_') + ' PASS', flush=True)
    denied = http(cluster.url + '/v1/config/llm?nodeId=embedded', token=cluster.users['alice']['access_token'])
    require(denied.get('error', {}).get('code') == 'permissionDenied', 'An owner of another Computer acquired G private configuration')
    cluster.enroll_management_host('admin')
    require(http(cluster.url + '/v1/config/llm?nodeId=embedded', token=cluster.users['admin']['access_token'])['status'] == 'success',
            'G administrator cannot configure its own private Computer')
    print('CASE sync_center_private_configuration_requires_its_administrator PASS', flush=True)

    from turn_helpers import submit, settled
    node = cluster.desktops['Desktop-A']['node']
    session = str(uuid.uuid4())
    turn = submit(cluster, 'Desktop-A', node, session,
        'Call AskUserQuestion exactly once: header Mode, question Choose mode, options id plan label Plan description Explore, id direct label Direct description Execute. After answering reply WAITING_CONFIG_PROTECTED.')
    wait_for(lambda: (cluster.desktop('Desktop-A', 'session/active_turn', {'sessionId': session})['activeTurn'] or {}).get('state') == 'waitingForInput', 'Embedded Worker is waiting for input', 90)
    action = cluster.events('alice', session, turn['turnId'], 90)[-1]['payload']['data']['action']
    before = (cluster.desktops['Desktop-A']['home'] / 'conf/talos.yaml').read_bytes()
    for host, target in [('Desktop-A', 'embedded'), ('Desktop-A', node), ('Desktop-B', node)]:
        config = cluster.desktop(host, 'config/get', {'nodeId': target, 'path': 'llm'})['config']
        denied = cluster.desktops[host]['host'].request('config/update', {'nodeId': target, 'value': {'type': 'llm', 'value': config}}, cluster.desktops[host]['token'])
        require(denied.get('error', {}).get('code') in ('conflict', 'activeTurnRunning'), 'Waiting embedded Worker configuration was mutable through ' + host + ' ' + target)
    require((cluster.desktops['Desktop-A']['home'] / 'conf/talos.yaml').read_bytes() == before, 'Rejected waiting-state update changed Worker configuration')
    cluster.desktop('Desktop-A', 'turn/input', {'identity': {'sessionId': session, 'turnId': turn['turnId']},
        'input': {'type': 'userActionResponse', 'actionId': action['actionId'], 'response': {'type': 'question', 'response': {'type': 'selectedOption', 'optionId': 'plan'}}}, 'clientRequestId': str(uuid.uuid4())})
    wait_for(lambda: settled(cluster, 'Desktop-A', session, turn['turnId']), 'Protected continuation resumed on the same Worker', 120)
    print('CASE sync_waiting_owner_configuration_rejects_embedded_uuid_and_remote_paths PASS', flush=True)

    if os.environ.get('TALOS_E2E_REVIEW_UI') == '1':
        from review_ui import capture
        capture(cluster)
